//! Pandoc grid tables: the shapes that must become a table, and — just as
//! load-bearing — the ASCII that must not.
//!
//! Grid tables are the one block whose opening line, `+---+`, is also
//! perfectly ordinary prose. Half of this file exists to pin the negative
//! side of that line: box art, fenced content and indented code that the
//! default profile must leave exactly where CommonMark left it.
//!
//! Where a test can be checked against something other than the parser it
//! is: the "must not match" cases assert the *same* output the
//! CommonMark-only profile produces for the identical input, so the
//! oracle is the parser's own pre-existing, spec-conformant behavior
//! rather than a hand-copied expectation.

use ast::{Alignment, Block, BlockKind, Document, InlineKind, NodeRef, ParseOptions, Span};

/// Every `Table` block in the document, outermost first.
fn tables(doc: &Document) -> Vec<&Block> {
    doc.nodes()
        .filter_map(|n| match n {
            NodeRef::Block(b) if matches!(b.kind, BlockKind::Table { .. }) => Some(b),
            _ => None,
        })
        .collect()
}

/// The table's rows flattened to `(header, [cell text, …])`, with each
/// cell's inlines rendered back to plain text.
fn grid_of(table: &Block) -> Vec<(bool, Vec<String>)> {
    let BlockKind::Table { rows, .. } = &table.kind else {
        panic!("not a table");
    };
    rows.iter()
        .map(|row| {
            let BlockKind::TableRow { header, cells } = &row.kind else {
                panic!("table child is not a row");
            };
            let texts = cells
                .iter()
                .map(|cell| {
                    let BlockKind::TableCell { children } = &cell.kind else {
                        panic!("row child is not a cell");
                    };
                    children
                        .iter()
                        .map(|i| flatten(&i.kind))
                        .collect::<String>()
                })
                .collect();
            (*header, texts)
        })
        .collect()
}

fn flatten(kind: &InlineKind) -> String {
    match kind {
        InlineKind::Text(t) | InlineKind::Code(t) => t.clone(),
        InlineKind::SoftBreak => " ".into(),
        InlineKind::Link { children, .. }
        | InlineKind::Emph(children)
        | InlineKind::Strong(children) => children.iter().map(|i| flatten(&i.kind)).collect(),
        other => format!("{other:?}"),
    }
}

/// The first `Link` inline anywhere in the document, with its span.
fn first_link(doc: &Document) -> Option<(Span, String)> {
    doc.nodes().find_map(|n| match n {
        NodeRef::Inline(i) => match &i.kind {
            InlineKind::Link { dest, .. } => Some((i.span, dest.clone())),
            _ => None,
        },
        _ => None,
    })
}

/// The negative-case oracle: parsing `md` with grid tables on must produce
/// byte-identical HTML to parsing it with every extension off.
fn assert_unchanged_by_grid_tables(name: &str, md: &str) {
    let with_grid = ast::html::to_html(&Document::parse(md));
    let commonmark = ast::html::to_html(&Document::parse_with(md, &ParseOptions::commonmark()));
    assert_eq!(
        with_grid, commonmark,
        "{name}: the default profile changed what CommonMark makes of this input"
    );
    assert!(
        tables(&Document::parse(md)).is_empty(),
        "{name}: parsed to a table"
    );
}

#[test]
fn test_a_grid_table_with_a_header_rule_yields_a_header_row_and_two_body_rows() {
    let doc = Document::parse(concat!(
        "+---------------+---------------+\n",
        "| Header A      | Header B      |\n",
        "+===============+===============+\n",
        "| body a        | body b        |\n",
        "+---------------+---------------+\n",
        "| body c        | body d        |\n",
        "+---------------+---------------+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1, "one table");
    assert_eq!(
        grid_of(tables[0]),
        vec![
            (true, vec!["Header A".into(), "Header B".into()]),
            (false, vec!["body a".into(), "body b".into()]),
            (false, vec!["body c".into(), "body d".into()]),
        ]
    );
}

#[test]
fn test_a_grid_table_without_a_header_rule_has_no_header_row() {
    let doc = Document::parse(concat!(
        "+------+--------+\n",
        "| one  | first  |\n",
        "+------+--------+\n",
        "| two  | second |\n",
        "+------+--------+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1, "one table");
    let grid = grid_of(tables[0]);
    assert!(
        grid.iter().all(|(header, _)| !header),
        "a table with no `+===+` rule must have no header row: {grid:?}"
    );
    assert_eq!(grid.len(), 2, "both rows are body rows");
}

#[test]
fn test_a_grid_table_renders_the_same_html_as_the_equivalent_pipe_table() {
    // The strongest available oracle for "same BlockKind::Table": the GFM
    // pipe-table parser is a wholly separate code path, and the HTML shim
    // only sees the AST. Identical HTML means identical trees modulo spans.
    let grid = Document::parse(concat!(
        "+-------+-------+\n",
        "| a     | b     |\n",
        "+=======+=======+\n",
        "| c     | d     |\n",
        "+-------+-------+\n",
    ));
    let pipe = Document::parse("| a | b |\n| - | - |\n| c | d |\n");
    assert_eq!(ast::html::to_html(&grid), ast::html::to_html(&pipe));
}

#[test]
fn test_alignment_markers_on_the_header_separator_reach_the_table() {
    let doc = Document::parse(concat!(
        "+-------+-------+-------+-------+\n",
        "| l     | c     | r     | n     |\n",
        "+:======+:=====:+======:+=======+\n",
        "| a     | b     | c     | d     |\n",
        "+-------+-------+-------+-------+\n",
    ));
    let tables = tables(&doc);
    let BlockKind::Table { alignments, .. } = &tables[0].kind else {
        panic!("not a table");
    };
    assert_eq!(
        alignments,
        &vec![
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::None
        ]
    );
}

#[test]
fn test_a_headerless_grid_table_takes_its_alignment_from_the_top_rule() {
    // Pandoc reads alignment off the header separator; with no separator
    // there is only the top rule to read.
    let doc = Document::parse(concat!(
        "+:------+------:+\n",
        "| a     | b     |\n",
        "+-------+-------+\n",
    ));
    let tables = tables(&doc);
    let BlockKind::Table { alignments, .. } = &tables[0].kind else {
        panic!("not a table");
    };
    assert_eq!(alignments, &vec![Alignment::Left, Alignment::Right]);
}

#[test]
fn test_a_cell_written_over_three_lines_joins_with_single_spaces() {
    let doc = Document::parse(concat!(
        "+--------+------------+\n",
        "| single | one        |\n",
        "|        | two        |\n",
        "|        | three      |\n",
        "+--------+------------+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(
        grid_of(tables[0]),
        vec![(false, vec!["single".into(), "one two three".into()])]
    );
}

#[test]
fn test_a_blank_line_inside_a_cell_does_not_double_the_joining_space() {
    let doc = Document::parse(concat!(
        "+------------+\n",
        "| above      |\n",
        "|            |\n",
        "| below      |\n",
        "+------------+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(
        grid_of(tables[0]),
        vec![(false, vec!["above below".into()])]
    );
}

#[test]
fn test_a_link_straddling_two_cell_lines_spans_the_source_it_came_from() {
    let src = concat!(
        "+--------+-----------+\n",
        "| plain  | see [the  |\n",
        "|        | docs](/d) |\n",
        "+--------+-----------+\n",
    );
    let doc = Document::parse(src);
    let (span, dest) = first_link(&doc).expect("the link parses out of the joined cell");
    assert_eq!(dest, "/d");

    // The oracle is the source text itself: the span must start at the
    // `[` on the first cell line and end at the `)` on the second, so the
    // slice it names begins and ends with those exact bytes.
    let slice = &src[span.start..span.end];
    assert!(
        slice.starts_with("[the") && slice.ends_with("docs](/d)"),
        "link span names {slice:?}, which is not the link's source text"
    );
    assert_eq!(
        src[..span.start].matches('\n').count(),
        1,
        "the span must start on the first of the two cell lines"
    );
    assert_eq!(
        src[..span.end].matches('\n').count(),
        2,
        "the span must end on the second of the two cell lines"
    );
}

#[test]
fn test_hand_drawn_box_art_with_drifting_borders_stays_a_paragraph() {
    assert_unchanged_by_grid_tables(
        "flowchart",
        concat!(
            "+----------+\n",
            "|  parser  |\n",
            "+----+-----+\n",
            "     |\n",
            "     v\n",
            "   output\n",
        ),
    );
}

#[test]
fn test_a_diagram_whose_rule_line_has_gaps_stays_a_paragraph() {
    assert_unchanged_by_grid_tables(
        "two boxes and an arrow",
        concat!(
            "+------+      +------+\n",
            "| A    | ---> | B    |\n",
            "+------+      +------+\n",
        ),
    );
}

#[test]
fn test_a_rule_line_with_no_content_rows_stays_a_paragraph() {
    assert_unchanged_by_grid_tables("banner", "+--------+\n+--------+\n");
    assert_unchanged_by_grid_tables("lone rule", "+--------+\n");
}

#[test]
fn test_a_rule_line_inside_a_fenced_code_block_stays_literal() {
    let doc = Document::parse("```text\n+---+---+\n| a | b |\n+---+---+\n```\n");
    assert!(
        tables(&doc).is_empty(),
        "a fence's contents are never parsed"
    );
    let literal = doc
        .nodes()
        .find_map(|n| match n {
            NodeRef::Block(b) => match &b.kind {
                BlockKind::CodeBlock { literal, .. } => Some(literal.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("code block");
    assert_eq!(literal, "+---+---+\n| a | b |\n+---+---+\n");
}

#[test]
fn test_a_rule_line_indented_four_spaces_stays_indented_code() {
    let md = "    +---+\n    | a |\n    +---+\n";
    assert_unchanged_by_grid_tables("indented", md);
    let doc = Document::parse(md);
    let literal = doc
        .nodes()
        .find_map(|n| match n {
            NodeRef::Block(b) => match &b.kind {
                BlockKind::CodeBlock {
                    literal,
                    fenced: false,
                    ..
                } => Some(literal.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("indented code block");
    assert_eq!(literal, "+---+\n| a |\n+---+\n");
}

#[test]
fn test_a_rule_line_directly_under_paragraph_text_does_not_interrupt_it() {
    // Pandoc wants a blank line ahead of a grid table, and refusing to
    // interrupt is what keeps prose that merely mentions `+--+` intact.
    assert_unchanged_by_grid_tables(
        "no blank line",
        "some prose\n+---+---+\n| a | b |\n+---+---+\n",
    );
}

#[test]
fn test_the_commonmark_profile_does_not_parse_grid_tables() {
    let md = concat!(
        "+-------+-------+\n",
        "| a     | b     |\n",
        "+=======+=======+\n",
        "| c     | d     |\n",
        "+-------+-------+\n",
    );
    assert_eq!(tables(&Document::parse(md)).len(), 1, "on by default");
    let cm = Document::parse_with(md, &ParseOptions::commonmark());
    assert!(
        tables(&cm).is_empty(),
        "ParseOptions::commonmark() must leave grid tables as text"
    );

    // The gate is its own flag, not GFM's: turning pipe tables off must
    // not take grid tables with it, and vice versa.
    let mut only_grid = ParseOptions::commonmark();
    only_grid.grid_tables = true;
    assert_eq!(tables(&Document::parse_with(md, &only_grid)).len(), 1);
    let no_grid = ParseOptions {
        grid_tables: false,
        ..ParseOptions::default()
    };
    assert!(tables(&Document::parse_with(md, &no_grid)).is_empty());
}

#[test]
fn test_a_grid_table_ends_at_its_last_rule_and_the_document_continues() {
    let doc = Document::parse(concat!(
        "+-----+\n",
        "| a   |\n",
        "+-----+\n",
        "\n",
        "after the table\n",
    ));
    assert_eq!(tables(&doc).len(), 1);
    let last = doc.blocks().last().expect("blocks");
    assert!(
        matches!(last.kind, BlockKind::Paragraph { .. }),
        "the paragraph after the table must survive: {:?}",
        last.kind
    );
}

#[test]
fn test_a_table_left_unclosed_at_end_of_input_keeps_the_part_that_closed() {
    // `| dangling |` has no rule under it, so it is not part of the table;
    // the rows that *are* fenced by rules still parse.
    let doc = Document::parse(concat!(
        "+-----------+\n",
        "| closed    |\n",
        "+-----------+\n",
        "| dangling  |\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["closed".into()])]);
    let last = doc.blocks().last().expect("blocks");
    assert!(
        matches!(last.kind, BlockKind::Paragraph { .. }),
        "the unfenced row falls out as a paragraph: {:?}",
        last.kind
    );
}

#[test]
fn test_a_cell_holding_only_a_bar_keeps_it_as_literal_text() {
    // Grid cells have no `\|` escape: a `|` that is not sitting in a
    // corner column is content.
    let doc = Document::parse("+---+---+\n| | | | |\n+---+---+\n");
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(
        grid_of(tables[0]),
        vec![(false, vec!["|".into(), "|".into()])]
    );
}

#[test]
fn test_a_fence_drawn_inside_a_cell_is_flattened_to_its_literal_text() {
    // The documented ceiling: `TableCell` holds inlines, so a block-level
    // construct inside a cell cannot survive as one. The three lines join
    // into "``` code ```", which the inline phase then reads as a code
    // *span* — never as the fenced block the author drew.
    let doc = Document::parse(concat!(
        "+-------------+\n",
        "| ```         |\n",
        "| code        |\n",
        "| ```         |\n",
        "+-------------+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["code".into()])]);
    assert!(
        doc.nodes().any(|n| matches!(
            n,
            NodeRef::Inline(i) if matches!(&i.kind, InlineKind::Code(c) if c == "code")
        )),
        "the fence collapsed into an inline code span"
    );
    assert!(
        !doc.nodes().any(|n| matches!(
            n,
            NodeRef::Block(b) if matches!(b.kind, BlockKind::CodeBlock { .. })
        )),
        "a cell cannot contain a block"
    );
}

#[test]
fn test_a_grid_table_under_three_spaces_of_indent_still_parses() {
    // Three spaces is the CommonMark limit for a block start; the fourth
    // is indented code, which the neighbouring test pins.
    let doc = Document::parse(concat!(
        "   +-----+-----+\n",
        "   | a   | b   |\n",
        "   +=====+=====+\n",
        "   | c   | d   |\n",
        "   +-----+-----+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(
        grid_of(tables[0]),
        vec![
            (true, vec!["a".into(), "b".into()]),
            (false, vec!["c".into(), "d".into()]),
        ]
    );
}

#[test]
fn test_a_grid_table_whose_lines_do_not_share_one_indent_is_not_a_table() {
    // The indent is part of the shape: a rule that moved sideways is the
    // same failure as a corner that moved.
    assert_unchanged_by_grid_tables("drifting indent", "  +-----+\n  | a   |\n   +-----+\n");
}

#[test]
fn test_a_grid_table_inside_a_list_item_parses_when_the_prefix_is_spaces() {
    let doc = Document::parse(concat!(
        "- item\n",
        "\n",
        "  +-----+\n",
        "  | a   |\n",
        "  +-----+\n",
    ));
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1, "the table belongs to the list item");
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["a".into()])]);
    assert!(
        matches!(
            doc.blocks().first().map(|b| &b.kind),
            Some(BlockKind::List { .. })
        ),
        "and the list still wraps it"
    );
}

#[test]
fn test_a_grid_table_inside_a_blockquote_is_not_recognised() {
    // A documented gap, not a silent one: `grid::scan` measures the
    // table's indent off raw source lines, and `> ` is not indent.
    let md = "> +-----+\n> | a   |\n> +-----+\n";
    let doc = Document::parse(md);
    assert!(tables(&doc).is_empty());
    assert_eq!(
        ast::html::to_html(&doc),
        ast::html::to_html(&Document::parse_with(md, &ParseOptions::commonmark())),
        "it stays exactly the paragraph CommonMark makes of it"
    );
}

#[test]
fn test_a_grid_table_written_with_crlf_line_endings_parses_the_same() {
    // The scan reads raw source lines, so it owns its own line splitting
    // and has to agree with the block parser's about `\r\n`.
    let lf = "+---+---+\n| a | b |\n+===+===+\n| c | d |\n+---+---+\n";
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(
        ast::html::to_html(&Document::parse(&crlf)),
        ast::html::to_html(&Document::parse(lf)),
    );
}

#[test]
fn test_a_tab_inside_a_cell_costs_the_line_its_table() {
    // Tabs are not expanded before the columns are counted, and a tab is
    // one character where the border geometry expects several. Rather than
    // silently mis-slicing the row, the line fails to be a content row and
    // the whole construct stays prose — the safe direction to fail in.
    assert_unchanged_by_grid_tables("tabbed cell", "+---+\n|\ta|\n+---+\n");
}

// ---------------------------------------------------------------------------
// Boundary cases the low-level scan helpers own.
//
// `grid::scan` walks raw source lines itself instead of borrowing the block
// parser's line loop, so it carries its own copies of three fiddly
// decisions: where a line ends, what counts as this table's indent, and
// which characters bound a cell. A mutation run over the module found those
// three helpers under-covered — every table in the tests above is
// well-formed, sits at column zero, ends in a newline and has no empty
// cell, so the edges of each helper were never reached. The tests below
// walk each edge from the outside, through `Document::parse`.
// ---------------------------------------------------------------------------

#[test]
fn test_a_grid_table_that_ends_at_end_of_input_without_a_newline_still_closes() {
    // The closing rule is the last line and the file has no final newline,
    // so the line walk runs off the end of the buffer rather than stopping
    // at a terminator. That is the only path that reaches the end-of-input
    // branch, and every other table in this file ends in "\n".
    let src = "+---+\n| a |\n+---+";
    assert!(!src.ends_with('\n'), "the missing newline is the point");
    let doc = Document::parse(src);
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["a".into()])]);
}

#[test]
fn test_the_blank_line_after_a_closing_rule_belongs_to_the_document_not_the_table() {
    // The oracle is the source text: whatever the table's span covers must
    // read back as exactly the table, ending on the closing rule's own `+`.
    // A line walk that mistook the blank line's terminator for part of the
    // rule's would hand the table a span one line too long.
    let src = "+---+\n| a |\n+---+\n\nafter\n";
    let doc = Document::parse(src);
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    let span = tables[0].span;
    assert_eq!(
        &src[span.start..span.end],
        "+---+\n| a |\n+---+",
        "the table's span must stop at its closing rule"
    );
    let last = doc.blocks().last().expect("blocks");
    assert!(
        matches!(last.kind, BlockKind::Paragraph { .. }),
        "the paragraph past the blank line is its own block: {:?}",
        last.kind
    );
}

#[test]
fn test_a_line_shorter_than_the_table_indent_ends_the_table_without_panicking() {
    // An indented table followed by a blank line: the blank line is
    // *shorter* than the indent the table established, which is the one
    // case where reading the indent columns off the line would run past
    // the line's own end.
    let src = "   +---+\n   | a |\n   +---+\n\nx\n";
    let doc = Document::parse(src);
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["a".into()])]);
    let last = doc.blocks().last().expect("blocks");
    assert!(
        matches!(last.kind, BlockKind::Paragraph { .. }),
        "the short line after the table is ordinary prose: {:?}",
        last.kind
    );
}

#[test]
fn test_text_written_into_the_indent_columns_breaks_the_table() {
    // The indent is not merely skipped, it is *checked*: a line long
    // enough to reach the borders but carrying text where the indent
    // should be is not part of this table, which leaves the construct
    // without a closing rule and therefore not a table at all.
    assert_unchanged_by_grid_tables("text in the indent", "  +---+\n  | a |\nx | b |\n  +---+\n");
}

#[test]
fn test_the_smallest_grid_table_is_one_column_one_character_wide() {
    // `+-+` is three characters — the shortest string that can be a rule
    // at all. Every other table here is far wider, so nothing else pins
    // the lower bound.
    let doc = Document::parse("+-+\n|a|\n+-+\n");
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1, "`+-+` is a rule");
    assert_eq!(grid_of(tables[0]), vec![(false, vec!["a".into()])]);
}

#[test]
fn test_a_rule_that_stops_short_of_a_closing_corner_is_not_a_rule() {
    // `+---+--` has a corner at the start and one in the middle but none
    // at the end. Reading it as a two-corner rule and ignoring the tail
    // would turn this into a one-cell table; a rule has to close.
    assert_unchanged_by_grid_tables("unclosed rule", "+---+--\n| a |\n+---+--\n");
}

#[test]
fn test_an_empty_cell_gets_a_zero_width_span_just_inside_its_own_border() {
    // An empty cell has no content to take a span from, so it falls back
    // to an anchor — and an anchor placed anywhere but immediately after
    // the cell's own left border would put the cell outside its row or
    // outside the document. The oracle is the source: the byte before the
    // span must be the `|` that opens the cell.
    let src = "+-----+-----+\n| a   |     |\n+-----+-----+\n";
    let doc = Document::parse(src);
    let tables = tables(&doc);
    assert_eq!(tables.len(), 1);
    let BlockKind::Table { rows, .. } = &tables[0].kind else {
        panic!("not a table");
    };
    let BlockKind::TableRow { cells, .. } = &rows[0].kind else {
        panic!("not a row");
    };
    assert_eq!(cells.len(), 2);
    let empty = cells[1].span;
    assert!(
        empty.end <= src.len(),
        "the empty cell's span {empty:?} escapes a {}-byte document",
        src.len()
    );
    assert_eq!(empty.start, empty.end, "an empty cell spans no source");
    assert!(
        empty.start >= rows[0].span.start && empty.end <= rows[0].span.end,
        "the empty cell's span {empty:?} escapes its row {:?}",
        rows[0].span
    );
    assert_eq!(
        &src[empty.start - 1..empty.start],
        "|",
        "the anchor sits immediately after the cell's opening border"
    );
}
