//! Pandoc grid tables (extension).
//!
//! ```text
//! +---------------+---------------+
//! | Header A      | Header B      |
//! +===============+===============+
//! | body a        | body b        |
//! +---------------+---------------+
//! ```
//!
//! A grid table lowers into exactly the same [`PreKind::Table`] the GFM
//! pipe-table parser produces, so everything downstream — layout, HTML,
//! decor — is unchanged. The only thing this module owns is turning a
//! rectangle of ASCII into rows and cells.
//!
//! # Why this is a whole-table lookahead and not a line-by-line block
//!
//! Every other block in [`super::block`] can be recognized from its first
//! line alone: `> `, `#`, ```` ``` ```` and friends are unambiguous the
//! moment you see them. `+---+` is not. It is also perfectly ordinary
//! prose — the top edge of a hand-drawn diagram, an ASCII banner, a
//! `tree` listing. Committing to a table on the first line would silently
//! swallow all of that under the default profile, so instead
//! [`scan`] validates the *entire* table before the block parser opens
//! anything: every rule line must repeat the first rule's corner columns
//! exactly, every content line must carry a `|` in each of those columns
//! and end at the last one, and the whole thing must close with a rule.
//! Box art that is merely table-*shaped* fails one of those and stays
//! paragraph text.
//!
//! Because the scan parses the table outright, the block parser has
//! nothing left to do per line: it opens a [`super::block::OpenKind`]
//! carrying the finished rows and swallows lines until it passes
//! [`GridTable::end`].
//!
//! # Cost
//!
//! Scanning the whole table from every line that starts with `+` sounds
//! quadratic. It is not, and the reason is worth writing down because the
//! obvious "cheap first-line gate" defence is not what saves it.
//!
//! Two candidate rules can only cost twice if their scans overlap, and
//! they cannot:
//!
//! - A *successful* scan opens a block that swallows every line through
//!   the closing rule, so those lines are never offered to an opener
//!   again. Whatever the scan read past that rule is content rows — no
//!   `+`, so no candidate — until the run ends.
//! - A *failing* scan means the run it read had no closing rule or no
//!   content row. If a later candidate sat inside that run it would be a
//!   second rule line, which leaves "no content row" as the only reason to
//!   fail — so the whole run is rule lines. Rule lines are ordinary
//!   paragraph text, the first one opens a paragraph, and the arm in
//!   [`super::block`] refuses to interrupt a paragraph. The later rules
//!   are therefore never candidates at all.
//!
//! So the runs partition the document and the total work is linear.
//! `crates/ast/tests/hardening.rs` holds the shapes this argument covers,
//! measured rather than assumed.
//!
//! # Documented ceiling: cells are inline-flattened
//!
//! [`crate::BlockKind::TableCell`] holds `Vec<Inline>`, not blocks. A
//! grid cell that spans N source lines is therefore joined into one
//! inline run — the per-line fragments are stripped of their padding and
//! separated by a single space — and parsed as inlines. Pandoc's
//! multi-block cells (a list, a nested fence, two paragraphs inside one
//! cell) are *not* supported. A fence drawn inside a cell joins into
//! ```` ``` code ``` ```` and comes back out as a code *span*, never as
//! the block the author meant. Row spans and column spans are not
//! supported either. Widening the AST to fix that is a deliberate
//! non-goal here, not an oversight.
//!
//! One more boundary worth stating out loud: the scan reads *raw source
//! lines*, so the only container prefix it can see through is spaces. A
//! grid table indented by up to three spaces parses, and so does one
//! inside a list item whose content prefix is plain spaces — but one
//! inside a blockquote does not, because `> ` is not indent the scan can
//! strip. `crates/ast/tests/grid_tables.rs` pins all three.

use super::{Leaf, PreRow};
use crate::ast::{Alignment, Span};

/// A validated grid table, parsed in full by [`scan`].
pub(crate) struct GridTable {
    pub alignments: Vec<Alignment>,
    pub rows: Vec<PreRow>,
    /// Offset of the first byte *after* the closing rule line, terminator
    /// included. The block parser keeps the table open for every line that
    /// begins before this.
    pub end: usize,
}

/// One rule line's geometry: `+---+===+` style.
struct Rule {
    /// Char offsets of the `+` corners, measured from the first character
    /// after the common indent. Char offsets, not bytes, because content
    /// rows may hold multi-byte text and the columns still have to line up.
    cols: Vec<usize>,
    /// Segments are `=` rather than `-`: Pandoc's header separator.
    heavy: bool,
    /// One entry per column, from the `:` markers in the segments.
    aligns: Vec<Alignment>,
}

/// What one line of a candidate table turned out to be.
enum Kind {
    Rule { heavy: bool, aligns: Vec<Alignment> },
    Body,
}

/// One accepted line: its kind plus the absolute byte range of its
/// content, indent stripped from the front and padding from the back.
struct Entry {
    kind: Kind,
    start: usize,
    end: usize,
}

/// Validate and parse a grid table whose top rule begins the line at
/// `line_start`. Returns `None` — leaving the caller to treat the line as
/// ordinary text — unless the whole shape holds together.
pub(crate) fn scan(src: &str, line_start: usize) -> Option<GridTable> {
    let (_, first_end, _) = next_line(src, line_start)?;
    let first = &src[line_start..first_end];
    // The indent is fixed by the top rule and every later line must repeat
    // it byte-for-byte. Only spaces count: a tab would make the char
    // columns below mean two different things on two different lines.
    let indent = first.len() - first.trim_start_matches(' ').len();
    let top = scan_rule(first[indent..].trim_end_matches([' ', '\t']))?;

    let mut entries: Vec<Entry> = Vec::new();
    let mut ends: Vec<usize> = Vec::new(); // byte just past each line's terminator
    let mut pos = line_start;
    while let Some((s, e, next)) = next_line(src, pos) {
        let Some((body, body_start)) = line_body(src, s, e, indent) else {
            break;
        };
        let kind = if let Some(rule) = scan_rule(body) {
            // A rule whose corners moved is not part of *this* table. This
            // is the check that rejects ragged hand-drawn box art.
            if rule.cols != top.cols {
                break;
            }
            Kind::Rule {
                heavy: rule.heavy,
                aligns: rule.aligns,
            }
        } else if is_row_line(body, &top.cols) {
            Kind::Body
        } else {
            break;
        };
        entries.push(Entry {
            kind,
            start: body_start,
            end: body_start + body.len(),
        });
        ends.push(next);
        pos = next;
    }

    // The table ends at its last rule. Content rows trailing off the end
    // (an unclosed table at EOF) are left to the rest of the document
    // rather than dragging the whole construct down with them.
    let last_rule = entries
        .iter()
        .rposition(|e| matches!(e.kind, Kind::Rule { .. }))?;
    entries.truncate(last_rule + 1);
    let end = ends[last_rule];

    let n_rules = entries
        .iter()
        .filter(|e| matches!(e.kind, Kind::Rule { .. }))
        .count();
    if n_rules < 2 || !entries.iter().any(|e| matches!(e.kind, Kind::Body)) {
        // A lone rule, or two rules with nothing between them, is a
        // horizontal line someone drew — not a table.
        return None;
    }

    // Pandoc reads column alignment off the header separator when there is
    // one, and off the top rule otherwise.
    let alignments = entries
        .iter()
        .find_map(|e| match &e.kind {
            Kind::Rule {
                heavy: true,
                aligns,
            } => Some(aligns.clone()),
            _ => None,
        })
        .unwrap_or(top.aligns);

    let mut rows = Vec::new();
    let mut group: Vec<(usize, usize)> = Vec::new();
    for entry in &entries {
        match &entry.kind {
            Kind::Body => group.push((entry.start, entry.end)),
            Kind::Rule { heavy, .. } => {
                if !group.is_empty() {
                    // Only the *first* group can be the header, and only
                    // when a `=` rule closes it: that is Pandoc's rule, and
                    // it is what lets a table with no `=` anywhere come out
                    // with no header row at all.
                    let header = *heavy && rows.is_empty();
                    rows.push(build_row(src, &group, &top.cols, header));
                    group.clear();
                }
            }
        }
    }

    Some(GridTable {
        alignments,
        rows,
        end,
    })
}

/// `src[pos..]`'s first line as (content start, content end, next line
/// start). `None` at end of input.
fn next_line(src: &str, pos: usize) -> Option<(usize, usize, usize)> {
    if pos >= src.len() {
        return None;
    }
    let b = src.as_bytes();
    let mut e = pos;
    while e < b.len() && b[e] != b'\n' && b[e] != b'\r' {
        e += 1;
    }
    let next = if e >= b.len() {
        e
    } else if b[e] == b'\r' && b.get(e + 1) == Some(&b'\n') {
        e + 2
    } else {
        e + 1
    };
    Some((pos, e, next))
}

/// Strip the table's common indent and any trailing padding from the line
/// `src[start..end]`, returning the remaining text and its absolute start.
/// `None` when the line does not carry the exact indent.
fn line_body(src: &str, start: usize, end: usize, indent: usize) -> Option<(&str, usize)> {
    let line = &src[start..end];
    if line.len() < indent || !line.as_bytes()[..indent].iter().all(|&c| c == b' ') {
        return None;
    }
    Some((line[indent..].trim_end_matches([' ', '\t']), start + indent))
}

/// Parse a rule line: `+`, then one or more `-`/`=` segments (each
/// optionally flanked by `:` alignment markers) separated and closed by
/// `+`. Every segment must use the same fill character, so `+---+===+` is
/// not a rule.
fn scan_rule(s: &str) -> Option<Rule> {
    let b = s.as_bytes();
    if b.len() < 3 || b[0] != b'+' || b[b.len() - 1] != b'+' {
        return None;
    }
    let mut cols = vec![0usize];
    let mut aligns = Vec::new();
    let mut heavy: Option<bool> = None;
    let mut seg_start = 1;
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'+' => {
                let (align, seg_heavy) = scan_rule_segment(&s[seg_start..i])?;
                if *heavy.get_or_insert(seg_heavy) != seg_heavy {
                    return None;
                }
                aligns.push(align);
                cols.push(i);
                seg_start = i + 1;
                i += 1;
            }
            // Anything else — including the lead byte of a multi-byte
            // character — disqualifies the line, so `seg_start..i` above is
            // always a char boundary.
            b'-' | b'=' | b':' => i += 1,
            _ => return None,
        }
    }
    Some(Rule {
        cols,
        heavy: heavy?,
        aligns,
    })
}

/// One `---` / `:==:` run between two corners → its alignment and whether
/// it is a `=` (header separator) segment.
fn scan_rule_segment(seg: &str) -> Option<(Alignment, bool)> {
    let left = seg.starts_with(':');
    let rest = if left { &seg[1..] } else { seg };
    let right = rest.ends_with(':');
    let core = if right { &rest[..rest.len() - 1] } else { rest };
    let fill = *core.as_bytes().first()?;
    if fill != b'-' && fill != b'=' {
        return None;
    }
    if !core.bytes().all(|c| c == fill) {
        return None;
    }
    let align = match (left, right) {
        (true, true) => Alignment::Center,
        (true, false) => Alignment::Left,
        (false, true) => Alignment::Right,
        (false, false) => Alignment::None,
    };
    Some((align, fill == b'='))
}

/// Is `s` a content row for a rule with corners at `cols`? Every corner
/// column must hold a `|` and the line must stop at the last one. That
/// exactness is the second half of the box-art defence: a rule line with
/// spaces in it (`+--+  +--+`) never parses as a rule at all, and a
/// diagram whose bars sit anywhere but under the corners fails here.
fn is_row_line(s: &str, cols: &[usize]) -> bool {
    let map = char_offsets(s);
    let n_chars = map.len() - 1;
    if n_chars != cols.last().map_or(0, |c| c + 1) {
        return false;
    }
    cols.iter().all(|&c| s.as_bytes()[map[c]] == b'|')
}

/// Byte offset of every char in `s`, plus a final sentinel at `s.len()`.
fn char_offsets(s: &str) -> Vec<usize> {
    let mut v: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    v.push(s.len());
    v
}

/// Assemble one table row from the group of content lines between two
/// rules.
///
/// The per-column fragments are joined with a single space. Each fragment
/// is `push`ed into the cell's [`Leaf`] with its own real source offset and
/// length, so an inline that starts on one line of the cell and ends on the
/// next still maps back to the bytes it came from. The joining space is
/// pushed as a zero-length source segment pinned to the end of the previous
/// fragment, which is the same degradation [`Leaf::map`] already applies to
/// tab expansion and escaped pipes: positions inside it collapse to that
/// boundary instead of inventing an offset.
fn build_row(src: &str, group: &[(usize, usize)], cols: &[usize], header: bool) -> PreRow {
    let maps: Vec<Vec<usize>> = group
        .iter()
        .map(|&(s, e)| char_offsets(&src[s..e]))
        .collect();
    let mut cells = Vec::with_capacity(cols.len().saturating_sub(1));
    for j in 0..cols.len() - 1 {
        // The cell occupies the characters strictly between two corners.
        let (lo, hi) = (cols[j] + 1, cols[j + 1]);
        let anchor = group[0].0 + maps[0][lo];
        let mut leaf = Leaf::default();
        let mut first_start = None;
        let mut last_end = anchor;
        for (line, map) in group.iter().zip(&maps) {
            let raw = &src[line.0 + map[lo]..line.0 + map[hi]];
            let lead = raw.len() - raw.trim_start_matches([' ', '\t']).len();
            let text = raw.trim_matches([' ', '\t']);
            if text.is_empty() {
                // A blank line inside a cell adds nothing: no fragment and
                // no joining space, so `a\n\nb` in a cell reads as `a b`.
                continue;
            }
            let start = line.0 + map[lo] + lead;
            if !leaf.content.is_empty() {
                leaf.push(" ", last_end, 0);
            }
            leaf.push(text, start, text.len());
            first_start.get_or_insert(start);
            last_end = start + text.len();
        }
        let span = match first_start {
            Some(s) => Span::new(s, last_end),
            None => Span::new(anchor, anchor),
        };
        cells.push((span, leaf));
    }
    PreRow {
        span: Span::new(group[0].0, group[group.len() - 1].1),
        header,
        cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_rule_line_mixing_dashes_and_equals_is_not_a_rule() {
        // The oracle is Pandoc's own grammar: a separator line is drawn
        // with one fill character throughout. Mixing them is box art.
        assert!(scan_rule("+---+===+").is_none());
        assert!(scan_rule("+---+---+").is_some());
        assert!(scan_rule("+===+===+").is_some());
    }

    #[test]
    fn test_a_rule_segment_of_only_colons_has_no_fill_and_is_rejected() {
        assert!(scan_rule("+:+").is_none());
        assert!(scan_rule("+::+").is_none());
        assert!(scan_rule("+:-:+").is_some());
    }

    #[test]
    fn test_a_rule_needs_a_corner_at_both_ends_and_three_characters_to_fit_them() {
        // The two bounds of the shape, pinned from both sides. `+-+` is the
        // shortest string with room for two corners and a segment between
        // them; two characters cannot hold a segment at all.
        assert!(scan_rule("+-+").is_some());
        assert!(scan_rule("++").is_none());
        // A corner at the start and one in the middle is not enough: the
        // trailing `-` means the rule never closes, and reading it as a
        // two-corner rule with an ignorable tail would accept box art that
        // merely begins like a table.
        assert!(scan_rule("+-+-").is_none());
        assert!(scan_rule("+---").is_none());
        assert!(scan_rule("-+-+").is_none());
    }

    #[test]
    fn test_rule_corner_columns_are_reported_in_characters() {
        let rule = scan_rule("+--+----+").expect("well-formed rule");
        assert_eq!(rule.cols, vec![0, 3, 8]);
        assert_eq!(rule.aligns.len(), 2);
    }

    #[test]
    fn test_a_content_row_must_carry_a_bar_in_every_corner_column() {
        // Corners at 0, 3 and 8 — nine characters, bars at both ends and
        // one in the middle.
        let cols = vec![0, 3, 8];
        assert!(is_row_line("| a| b  |", &cols));
        // Same length, but the middle border drifted one column right.
        assert!(!is_row_line("| a | b |", &cols));
        // Correct borders, but the line runs past the last corner.
        assert!(!is_row_line("| a| b  | x", &cols));
        // Correct borders, but the line stops short of it.
        assert!(!is_row_line("| a| b |", &cols));
    }

    #[test]
    fn test_a_multi_byte_cell_shifts_bytes_but_not_columns() {
        // "é" is two bytes and one character: a byte-indexed check would
        // look for the middle bar one place too far to the left and give
        // up on every row that holds non-ASCII text.
        let cols = vec![0, 3, 8];
        assert!(is_row_line("|é | b  |", &cols));
    }
}
