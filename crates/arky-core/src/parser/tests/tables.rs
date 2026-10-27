use super::{check, check_spans};
use crate::parser::extension::Markdown;
use crate::parser::located::Block;
use crate::parser::{ParseOptions, parse_located};

/// A simple table whose columns show the four alignments.
const SIMPLE_TABLE: &str = "  Right     Left     Center     Default
-------     ------ ----------   -------
     12     12        12            12
    123     123       123          123
      1     1          1             1
";

const SIMPLE_TABLE_TREE: &str = r#"Document
  MarkdownBlock
    Section
      Table format=Simple alignments=Right,Left,Center,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Right"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Left"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Center"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Default"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "12"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "12"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "12"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "12"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "123"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "123"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "123"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "123"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
"#;

#[test]
fn a_pipe_table_reads_alignments_escaped_pipes_and_empty_cells() {
    check(
        r"| Name | Role | Notes |
|:-----|:----:|------:|
| Ada | host | uses `parse` and a \| pipe |
| Grace |  | [[review]] |
After the table.
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Left,Center,Right
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Name"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Role"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Notes"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Ada"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "host"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "uses "
                    Code "parse"
                    Text " and a | pipe"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Grace"
          TableCell
            MarkdownBlock
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Link form=Wiki target="review"
      Paragraph
        Line
          Text "After the table."
"#,
    );
}

#[test]
fn a_pipe_table_may_leave_out_its_outer_pipes() {
    check(
        "\
a | b
--|--
1 | 2
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
"#,
    );
}

#[test]
fn a_pipe_row_of_another_width_widens_the_table_or_gets_empty_cells() {
    check(
        "\
| a | b |
|---|---|
| 1 | 2 | 3 |
| 4 |
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default,Default,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
          TableCell
            MarkdownBlock
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "3"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "4"
          TableCell
            MarkdownBlock
          TableCell
            MarkdownBlock
"#,
    );
}

#[test]
fn a_dash_line_of_another_width_still_makes_a_table() {
    check(
        "\
| a | b |
|---|---|---|
| 1 | 2 |
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default,Default,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
          TableCell
            MarkdownBlock
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
          TableCell
            MarkdownBlock
"#,
    );
}

#[test]
fn a_column_the_dash_line_does_not_reach_has_the_default_alignment() {
    check(
        "\
| a |
|:-:|
| 1 | 2 |
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Center,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
"#,
    );
}

#[test]
fn a_pipe_cell_holds_a_markdown_block_with_its_headings() {
    check(
        "\
| x |
|---|
| # Foo |
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "x"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Heading
                  Text "Foo"
"#,
    );
}

#[test]
fn a_line_with_no_pipe_or_a_header_with_no_dash_line_under_it_is_text() {
    check(
        "\
a | b
plain

| a | b |
| 1 | 2 |
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a | b"
        Line
          Text "plain"
      Paragraph
        Line
          Text "| a | b |"
        Line
          Text "| 1 | 2 |"
"#,
    );
}

#[test]
fn a_table_may_sit_inside_a_list_item() {
    check(
        "\
- Results:

  | x | y |
  |---|--:|
  | 1 | 2 |
- next
",
        r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "Results:"
              Table format=Pipe alignments=Default,Right
                TableRow head
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "x"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "y"
                TableRow
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "1"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "2"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "next"
"#,
    );
}

#[test]
fn a_grid_table_reads_its_header_and_the_alignments_of_its_separator() {
    check(
        "\
+-------+-------+
| Name  | Role  |
+:======+:=====:+
| Ada   | host  |
+-------+-------+
| Grace | review|
+-------+-------+
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Left,Center
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Name"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Role"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Ada"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "host"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Grace"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "review"
"#,
    );
}

#[test]
fn a_grid_row_may_run_over_several_lines() {
    check(
        "\
+-----+-----+
| a   | b   |
| c   | d   |
+-----+-----+
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
                  Line
                    Text "c"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
                  Line
                    Text "d"
"#,
    );
}

#[test]
fn a_grid_wall_out_of_line_leaves_every_character_in_a_cell() {
    check(
        "\
+-----+-----+
| a  | b    |
+-----+-----+

+-----+-----+
| a   | b   |
| c  | d    |
+-----+-----+
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a  |"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
                  Line
                    Text "c  |"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
                  Line
                    Text "d"
"#,
    );
}

#[test]
fn a_grid_cell_holds_a_markdown_block_over_its_lines() {
    check(
        "\
+-------+-----+
| - a   | x   |
| - b   |     |
|   - c |     |
+-------+-----+
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                List
                  Item
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "a"
                  Item
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "b"
                        List
                          Item
                            MarkdownBlock
                              Section
                                Paragraph
                                  Line
                                    Text "c"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "x"
"#,
    );
}

#[test]
fn a_fence_of_tildes_in_a_grid_cell_is_a_code_block_as_one_of_backticks_is() {
    for fence in ["```", "~~~"] {
        check(
            &format!(
                "\
+---------+---+
| {fence}text | x |
| code    |   |
| {fence}     |   |
+---------+---+
"
            ),
            r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                CodeBlock language="text" value="code"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "x"
"#,
        );
    }
}

#[test]
fn a_simple_table_takes_each_alignment_from_where_its_header_sits() {
    check(SIMPLE_TABLE, SIMPLE_TABLE_TREE);
}

#[test]
fn a_simple_row_that_does_not_fit_its_columns_keeps_every_word_in_a_cell() {
    check(
        "  Name    Role
  ----    ----
  Ada     host
  Grace Hopper reviews
X Linus   maintains
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Simple alignments=Default,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Name"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Role"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Ada"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "host"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "Grace Hopper"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "reviews"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "X Linus"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "maintains"
"#,
    );
}

#[test]
fn a_simple_table_with_no_rows_is_not_a_table() {
    check(
        "\
Name    Role
----    ----
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Name    Role"
      ThematicBreak
"#,
    );
}

#[test]
fn a_simple_table_whose_lines_a_tab_indents_reads_as_one_that_spaces_indent() {
    for source in [
        "- x\n\n    a    b\n\t---  ---\n\t1    2\n",
        "- x\n\n    a    b\n    ---  ---\n    1    2\n",
    ] {
        check(
            source,
            r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "x"
              Table format=Simple alignments=Left,Left
                TableRow head
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "a"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "b"
                TableRow
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "1"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "2"
"#,
        );
    }
}

#[test]
fn a_tab_between_dash_groups_reaches_the_next_multiple_of_4_as_spaces_do() {
    for source in ["a    b\n---\t---\n1    2\n", "a    b\n--- ---\n1    2\n"] {
        check(
            source,
            r#"Document
  MarkdownBlock
    Section
      Table format=Simple alignments=Left,Center
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
"#,
        );
    }
}

#[test]
fn a_tab_in_a_simple_cell_stays_in_its_text() {
    check(
        "a\tx  b\n---  ---\n1    2\n",
        r#"Document
  MarkdownBlock
    Section
      Table format=Simple alignments=Left,Right
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a\tx"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "1"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
"#,
    );
}

#[test]
fn a_table_spans_from_its_first_line_to_its_last_and_a_cell_spans_its_text() {
    let whole = "| a | b |\n|---|---|\n| 1 |   |\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "| a | b |\n|---|---|\n| 1 |   |"),
            (
                "Table format=Pipe alignments=Default,Default",
                "| a | b |\n|---|---|\n| 1 |   |",
            ),
            ("TableRow head", "| a | b |"),
            ("TableCell", "a"),
            ("MarkdownBlock", "a"),
            ("Section", "a"),
            ("Paragraph", "a"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            ("TableCell", "b"),
            ("MarkdownBlock", "b"),
            ("Section", "b"),
            ("Paragraph", "b"),
            ("Line", "b"),
            (r#"Text "b""#, "b"),
            ("TableRow", "| 1 |   |"),
            ("TableCell", "1"),
            ("MarkdownBlock", "1"),
            ("Section", "1"),
            ("Paragraph", "1"),
            ("Line", "1"),
            (r#"Text "1""#, "1"),
            ("TableCell", ""),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn an_added_cell_is_empty_at_the_end_of_its_row() {
    let whole = "| a |\n|---|\n| 1 | 2 |\n";
    let table = "| a |\n|---|\n| 1 | 2 |";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", table),
            ("Table format=Pipe alignments=Default,Default", table),
            ("TableRow head", "| a |"),
            ("TableCell", "a"),
            ("MarkdownBlock", "a"),
            ("Section", "a"),
            ("Paragraph", "a"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            ("TableCell", ""),
            ("MarkdownBlock", ""),
            ("TableRow", "| 1 | 2 |"),
            ("TableCell", "1"),
            ("MarkdownBlock", "1"),
            ("Section", "1"),
            ("Paragraph", "1"),
            ("Line", "1"),
            (r#"Text "1""#, "1"),
            ("TableCell", "2"),
            ("MarkdownBlock", "2"),
            ("Section", "2"),
            ("Paragraph", "2"),
            ("Line", "2"),
            (r#"Text "2""#, "2"),
        ],
    );
    let document = parse_located::<Markdown>(whole, &ParseOptions::default());
    let block = &document.body.sections[0].blocks[0];
    assert!(matches!(block, Block::Table(read) if read.head[0].cells[1].span == (5..5)));
}

#[test]
fn a_grid_cell_spans_from_its_first_line_to_its_last() {
    let whole = "+---+---+\n| a | b |\n| c | d |\n+---+---+\n";
    let table = "+---+---+\n| a | b |\n| c | d |\n+---+---+";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", table),
            ("Table format=Grid alignments=Default,Default", table),
            ("TableRow", "| a | b |\n| c | d |"),
            ("TableCell", "a | b |\n| c"),
            ("MarkdownBlock", "a | b |\n| c"),
            ("Section", "a | b |\n| c"),
            ("Paragraph", "a | b |\n| c"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            ("Line", "c"),
            (r#"Text "c""#, "c"),
            ("TableCell", "b |\n| c | d"),
            ("MarkdownBlock", "b |\n| c | d"),
            ("Section", "b |\n| c | d"),
            ("Paragraph", "b |\n| c | d"),
            ("Line", "b"),
            (r#"Text "b""#, "b"),
            ("Line", "d"),
            (r#"Text "d""#, "d"),
        ],
    );
}

#[test]
fn a_definition_in_a_pipe_cell_is_a_definition_of_the_file() {
    check(
        "| a | b |\n|---|---|\n| [x]: /u | [x] |\n\n[x]\n",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default,Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
        TableRow
          TableCell
            MarkdownBlock
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Link form=Markdown target="/u"
                      Text "x"
      Paragraph
        Line
          Link form=Markdown target="/u"
            Text "x"
"#,
    );
}

#[test]
fn a_definition_in_a_grid_cell_is_a_definition_of_the_file() {
    check(
        "\
+---------+-----+
| [x]: /u | [x] |
+---------+-----+

[x]
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default,Default
        TableRow
          TableCell
            MarkdownBlock
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Link form=Markdown target="/u"
                      Text "x"
      Paragraph
        Line
          Link form=Markdown target="/u"
            Text "x"
"#,
    );
}

#[test]
fn a_definition_in_a_table_inside_a_grid_cell_is_a_definition_of_the_file() {
    check(
        "\
+---------------+
| | h |         |
| |---|         |
| | [y]: /v |   |
+---------------+

[y]
",
        r#"Document
  MarkdownBlock
    Section
      Table format=Grid alignments=Default
        TableRow
          TableCell
            MarkdownBlock
              Section
                Table format=Pipe alignments=Default
                  TableRow head
                    TableCell
                      MarkdownBlock
                        Section
                          Paragraph
                            Line
                              Text "h"
                  TableRow
                    TableCell
                      MarkdownBlock
      Paragraph
        Line
          Link form=Markdown target="/v"
            Text "y"
"#,
    );
}

#[test]
fn a_row_line_keeps_a_no_break_space_at_its_end_and_loses_its_spaces() {
    check(
        "| a |\n|---|\n| b\u{a0}\n",
        r#"Document
  MarkdownBlock
    Section
      Table format=Pipe alignments=Default
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
        TableRow
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b\u{a0}"
"#,
    );
    let whole = "| a |\n|---|\n| b  \n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "| a |\n|---|\n| b"),
            ("Table format=Pipe alignments=Default", "| a |\n|---|\n| b"),
            ("TableRow head", "| a |"),
            ("TableCell", "a"),
            ("MarkdownBlock", "a"),
            ("Section", "a"),
            ("Paragraph", "a"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            ("TableRow", "| b"),
            ("TableCell", "b"),
            ("MarkdownBlock", "b"),
            ("Section", "b"),
            ("Paragraph", "b"),
            ("Line", "b"),
            (r#"Text "b""#, "b"),
        ],
    );
}

#[test]
fn a_simple_table_on_an_items_first_line_leaves_the_marker_to_the_item() {
    for (source, list) in [
        ("- a    b\n  ---  ---\n  1    2\n", "List"),
        ("1. a    b\n   ---  ---\n   1    2\n", "List start=1"),
    ] {
        check(
            source,
            &format!(
                r#"Document
  MarkdownBlock
    Section
      {list}
        Item
          MarkdownBlock
            Section
              Table format=Simple alignments=Left,Left
                TableRow head
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "a"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "b"
                TableRow
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "1"
                  TableCell
                    MarkdownBlock
                      Section
                        Paragraph
                          Line
                            Text "2"
"#
            ),
        );
    }
}

#[test]
fn an_empty_first_cell_of_an_indented_simple_row_is_empty_at_the_start_of_its_row() {
    let source = "a    b\n---  ---\n       2\n";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      Table format=Simple alignments=Left,Left
        TableRow head
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "a"
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "b"
        TableRow
          TableCell
            MarkdownBlock
          TableCell
            MarkdownBlock
              Section
                Paragraph
                  Line
                    Text "2"
"#,
    );
    let document = parse_located::<Markdown>(source, &ParseOptions::default());
    let block = &document.body.sections[0].blocks[0];
    assert!(matches!(
        block,
        Block::Table(read)
            if read.rows[0].cells[0].span == (23..23) && read.rows[0].cells[1].span == (23..24)
    ));
}
