use core::iter;

use super::{Rows, check, on_two_megabytes, outline, valid, valid_body};
use crate::parser::ast::{Args, Block, Inline, Mapping, MarkdownBlock, RawDocument, Section};
use crate::parser::block;
use crate::parser::extension::Markdown;
use crate::parser::frontmatter::VALUE_CAP;
use crate::parser::inline::INLINE_CAP;
use crate::parser::lexer::BLOCK_CAP;
use crate::parser::{from_json, parse, to_json};
use crate::writer::{WriteOptions, write};

/// A container that `headed_containers` nests.
#[derive(Clone, Copy)]
enum Container {
    Grid,
    List,
    Markdown,
}

#[test]
fn a_paragraph_is_a_set_of_lines_and_a_blank_line_ends_it() {
    check(
        "\
First line
second line of the same paragraph.

A second paragraph.
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "First line"
        Line
          Text "second line of the same paragraph."
      Paragraph
        Line
          Text "A second paragraph."
"#,
    );
}

#[test]
fn a_heading_owns_what_follows_until_a_heading_of_its_level_or_higher() {
    check(
        "\
Intro before any heading.

# Alpha

Text in alpha.

## Beta

Text in beta.

### Gamma

## Delta

# Epsilon
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Intro before any heading."
    Section
      Heading
        Text "Alpha"
      Paragraph
        Line
          Text "Text in alpha."
      Section
        Heading
          Text "Beta"
        Paragraph
          Line
            Text "Text in beta."
        Section
          Heading
            Text "Gamma"
      Section
        Heading
          Text "Delta"
    Section
      Heading
        Text "Epsilon"
"#,
    );
}

#[test]
fn a_deeper_heading_opens_a_section_inside_the_current_one() {
    check(
        "\
# A
text

## D
text in D

# E
",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "A"
      Paragraph
        Line
          Text "text"
      Section
        Heading
          Text "D"
        Paragraph
          Line
            Text "text in D"
    Section
      Heading
        Text "E"
"#,
    );
}

#[test]
fn a_skipped_level_is_a_section_with_no_heading() {
    check(
        "# Hi H1\n### This is H3\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Hi H1"
      Section
        Section
          Heading
            Text "This is H3"
"#,
    );
}

#[test]
fn a_heading_line_with_no_text_is_a_heading_with_no_children() {
    check(
        "# A\n\n#\n\nfoo\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "A"
    Section
      Heading
      Paragraph
        Line
          Text "foo"
"#,
    );
    check(
        "# Hi H1\n##\n### This is H3\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Hi H1"
      Section
        Heading
        Section
          Heading
            Text "This is H3"
"#,
    );
    let source = "#\n\nfoo\n";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      Heading
      Paragraph
        Line
          Text "foo"
"#,
    );
    let written = write(&parse(source).document, &WriteOptions::default()).unwrap();
    assert_eq!(written, source);
}

#[test]
fn text_before_a_deeper_first_heading_holds_that_heading_as_a_subsection() {
    check(
        "intro\n\n## B\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "intro"
      Section
        Heading
          Text "B"
"#,
    );
}

#[test]
fn a_body_whose_first_heading_is_deeper_starts_with_a_section_with_no_heading() {
    check(
        "\
## Starts at two

# One

### Three

## Two
",
        r#"Document
  MarkdownBlock
    Section
      Section
        Heading
          Text "Starts at two"
    Section
      Heading
        Text "One"
      Section
        Section
          Heading
            Text "Three"
      Section
        Heading
          Text "Two"
"#,
    );
}

#[test]
fn a_heading_needs_one_to_six_hashes_and_a_space() {
    check(
        "\
#NoSpace is a paragraph

####### Seven hashes is a paragraph

#

## See [[grammar]] and `parse`
",
        "\
Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text \"#NoSpace is a paragraph\"
      Paragraph
        Line
          Text \"####### Seven hashes is a paragraph\"
    Section
      Heading
      Section
        Heading
          Text \"See \"
          Link form=Wiki target=\"grammar\"
          Text \" and \"
          Code \"parse\"
",
    );
}

#[test]
fn a_list_item_holds_sections_whose_levels_count_from_the_item() {
    check(
        "\
- # B
  in B
  # C
  in C
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Heading
                Text "B"
              Paragraph
                Line
                  Text "in B"
            Section
              Heading
                Text "C"
              Paragraph
                Line
                  Text "in C"
"#,
    );
}

#[test]
fn a_line_indented_to_the_item_text_continues_the_item() {
    check(
        "\
- one
- two
  continues two
- three
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "one"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "two"
                Line
                  Text "continues two"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "three"
"#,
    );
}

#[test]
fn a_line_left_of_the_item_text_ends_the_list_and_starts_a_paragraph() {
    for source in ["- a\nb\n", "- a\n b\n"] {
        check(
            source,
            r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "a"
      Paragraph
        Line
          Text "b"
"#,
        );
    }
}

#[test]
fn lists_nest_by_indentation_and_an_item_holds_several_blocks() {
    check(
        "\
- parent
  - child
    - grandchild
  - second child

  A second paragraph of parent.
- sibling
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
                  Text "parent"
              List
                Item
                  MarkdownBlock
                    Section
                      Paragraph
                        Line
                          Text "child"
                      List
                        Item
                          MarkdownBlock
                            Section
                              Paragraph
                                Line
                                  Text "grandchild"
                Item
                  MarkdownBlock
                    Section
                      Paragraph
                        Line
                          Text "second child"
              Paragraph
                Line
                  Text "A second paragraph of parent."
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "sibling"
"#,
    );
}

#[test]
fn another_bullet_character_starts_a_new_list_and_an_item_may_be_empty() {
    check(
        "\
* star

+ plus

-
- after the empty item
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "star"
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "plus"
      List
        Item
          MarkdownBlock
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "after the empty item"
"#,
    );
}

#[test]
fn a_line_left_of_the_item_text_ends_the_list() {
    check(
        "\
- item
continues at column zero
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "item"
      Paragraph
        Line
          Text "continues at column zero"
"#,
    );
}

#[test]
fn a_bullet_after_a_bullet_on_one_line_nests() {
    check(
        "- - inner\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              List
                Item
                  MarkdownBlock
                    Section
                      Paragraph
                        Line
                          Text "inner"
"#,
    );
}

#[test]
fn a_fenced_code_block_keeps_its_lines_as_they_are() {
    check(
        "\
```rust
fn main() {
    // # not a heading
    - not a list
}
```

After the code.
",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="rust" value="fn main() {\n    // # not a heading\n    - not a list\n}"
      Paragraph
        Line
          Text "After the code."
"#,
    );
}

#[test]
fn a_fence_inside_an_item_closes_only_on_a_fence_at_least_as_long() {
    check(
        "\
- step

  ````text
  ```
  inner fence

  stays code
  ````
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
                  Text "step"
              CodeBlock language="text" value="```\ninner fence\n\nstays code"
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
fn an_unclosed_fence_runs_to_the_end_of_the_file() {
    check(
        "\
```
never closed
# still code
",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="never closed\n# still code"
"#,
    );
}

#[test]
fn an_unclosed_fence_inside_an_item_ends_with_the_item() {
    check(
        "\
- item
  ```
  code
- next
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "item"
              CodeBlock value="code"
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
fn a_fence_indented_four_columns_is_content_and_one_indented_three_closes_the_block() {
    check(
        "```\ncode\n    ```\nafter\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="code\n    ```\nafter"
"#,
    );
    check(
        "```\ncode\n   ```\nafter\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="code"
      Paragraph
        Line
          Text "after"
"#,
    );
}

#[test]
fn a_fence_indented_four_columns_past_the_item_text_is_content() {
    check(
        "- x\n  ```\n  code\n        ```\n  after\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "x"
              CodeBlock value="code\n      ```\nafter"
"#,
    );
}

#[test]
fn a_list_or_a_heading_ends_a_paragraph_without_a_blank_line() {
    check(
        "\
Text line
- item
# Heading
text under the heading
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Text line"
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "item"
    Section
      Heading
        Text "Heading"
      Paragraph
        Line
          Text "text under the heading"
"#,
    );
}

#[test]
fn an_empty_file_is_a_document_with_an_empty_body() {
    check("", "Document\n  MarkdownBlock\n");
    check("\n\n", "Document\n  MarkdownBlock\n");
}

#[test]
fn the_item_text_starts_at_its_first_word() {
    check(
        "\
-   wide
  - inner
1.  one
    continues one
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "wide"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "inner"
      List start=1
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "one"
                Line
                  Text "continues one"
"#,
    );
}

#[test]
fn four_spaces_or_a_tab_of_indentation_are_one_code_block() {
    check(
        "    four spaces\n\n\t- tab\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="four spaces\n\n- tab"
"#,
    );
}

#[test]
fn a_tab_that_starts_a_line_reaches_column_four_and_starts_a_code_block() {
    check(
        "\tcode\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="code"
"#,
    );
}

#[test]
fn a_tab_past_the_code_column_stays_in_the_code() {
    check(
        "\tfoo\tbar\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock value="foo\tbar"
"#,
    );
}

#[test]
fn a_tab_after_the_hashes_starts_a_heading() {
    check(
        "#\tTitle\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Title"
"#,
    );
}

#[test]
fn a_tab_after_a_list_marker_starts_an_item() {
    check(
        "-\titem\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "item"
"#,
    );
}

#[test]
fn tabs_between_the_marks_of_a_thematic_break_keep_it_a_break() {
    check(
        "*\t*\t*\n",
        "Document
  MarkdownBlock
    Section
      ThematicBreak
",
    );
}

#[test]
fn a_line_a_tab_indents_to_the_item_text_continues_the_item() {
    check(
        "- foo\n\n\tbar\n",
        r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "foo"
              Paragraph
                Line
                  Text "bar"
"#,
    );
}

#[test]
fn a_tab_that_the_code_column_falls_inside_leaves_its_columns_past_it_as_spaces() {
    check(
        "- foo\n\n\t\tbar\n",
        r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "foo"
              CodeBlock value="  bar"
"#,
    );
}

#[test]
fn a_tab_before_an_html_block_past_the_item_text_is_kept_as_the_spaces_it_spans() {
    check(
        "1. j\n\n   \t<div>\n",
        r#"Document
  MarkdownBlock
    Section
      List start=1 loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "j"
              Html " <div>"
"#,
    );
}

#[test]
fn a_new_bullet_character_or_delimiter_starts_a_new_list() {
    check(
        "\
- one
* two
+ three

1. a
1) b
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "one"
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "two"
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "three"
      List start=1
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "a"
      List start=1
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "b"
"#,
    );
}

#[test]
fn an_ordered_list_keeps_its_first_number() {
    check(
        "\
3. third
4. fourth
",
        r#"Document
  MarkdownBlock
    Section
      List start=3
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "third"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "fourth"
"#,
    );
}

#[test]
fn an_ordered_marker_has_one_to_nine_digits() {
    check(
        "\
123456789) nine digits

1234567890) ten digits
",
        r#"Document
  MarkdownBlock
    Section
      List start=123456789
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "nine digits"
      Paragraph
        Line
          Text "1234567890) ten digits"
"#,
    );
}

#[test]
fn an_ordered_list_interrupts_a_paragraph_only_when_it_starts_at_one() {
    check(
        "\
text
2. two

text
1. one
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "text"
        Line
          Text "2. two"
      Paragraph
        Line
          Text "text"
      List start=1
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "one"
"#,
    );
}

#[test]
fn a_blank_line_between_items_or_between_two_blocks_of_an_item_makes_a_list_loose() {
    check(
        "\
- a

- b

# Next

- a
- b

  second block of b
- c

# Tight

- a
- b
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
                  Text "a"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "b"
    Section
      Heading
        Text "Next"
      List loose
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
              Paragraph
                Line
                  Text "second block of b"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "c"
    Section
      Heading
        Text "Tight"
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
"#,
    );
}

#[test]
fn a_blank_line_inside_a_nested_list_makes_only_the_list_it_separates_loose() {
    check(
        "\
- a
  - b

    c
- d
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "a"
              List loose
                Item
                  MarkdownBlock
                    Section
                      Paragraph
                        Line
                          Text "b"
                      Paragraph
                        Line
                          Text "c"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "d"
"#,
    );
}

#[test]
fn a_blank_line_closing_a_nested_list_separates_the_items_around_it() {
    check(
        "\
- a
  - b

- c
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
                  Text "a"
              List
                Item
                  MarkdownBlock
                    Section
                      Paragraph
                        Line
                          Text "b"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "c"
"#,
    );
}

#[test]
fn text_indented_four_columns_past_the_item_text_is_a_code_block() {
    check(
        "\
1. first
2. second

- item

      code here
",
        r#"Document
  MarkdownBlock
    Section
      List start=1
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "first"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "second"
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "item"
              CodeBlock value="code here"
"#,
    );
}

#[test]
fn a_marker_followed_by_five_spaces_starts_its_text_one_space_after_it() {
    check(
        "\
-     code
-      indented code
1.     indented code

   paragraph

       more code
",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              CodeBlock value="code"
        Item
          MarkdownBlock
            Section
              CodeBlock value=" indented code"
      List start=1 loose
        Item
          MarkdownBlock
            Section
              CodeBlock value="indented code"
              Paragraph
                Line
                  Text "paragraph"
              CodeBlock value="more code"
"#,
    );
}

#[test]
fn an_indented_code_block_keeps_the_blank_lines_between_its_lines() {
    check(
        "text\n\n    code\n\n      indented\n    \n\n# Heading\n    after the heading\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "text"
      CodeBlock value="code\n\n  indented"
    Section
      Heading
        Text "Heading"
      CodeBlock value="after the heading"
"#,
    );
}

#[test]
fn an_indented_line_after_paragraph_text_continues_the_paragraph() {
    check(
        "\
Some text
    more text
    # not a heading
    - not an item
",
        r##"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Some text"
        Line
          Text "more text"
        Line
          Text "# not a heading"
        Line
          Text "- not an item"
"##,
    );
}

#[test]
fn the_text_of_a_heading_ends_before_its_closing_hashes() {
    check(
        "# Title ##\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Title"
"#,
    );
    check(
        "### Title ###   \n",
        r#"Document
  MarkdownBlock
    Section
      Section
        Section
          Heading
            Text "Title"
"#,
    );
}

#[test]
fn a_heading_of_closing_hashes_alone_has_no_text() {
    check(
        "# #\n",
        "\
Document
  MarkdownBlock
    Section
      Heading
",
    );
    check(
        "### ###\n",
        "\
Document
  MarkdownBlock
    Section
      Section
        Section
          Heading
",
    );
}

#[test]
fn final_hashes_that_touch_the_text_or_follow_a_backslash_are_text() {
    check(
        "# Title#\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Title#"
"#,
    );
    check(
        "# Title \\#\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Title #"
"#,
    );
}

#[test]
fn a_fence_of_tildes_is_a_code_block() {
    for (source, language, value) in [
        ("~~~\ncode\n~~~\n", None, "code"),
        ("~~~python\nprint(1)\n~~~\n", Some("python"), "print(1)"),
        ("~~~ a`b\nx\n~~~\n", Some("a`b"), "x"),
    ] {
        check(source, &code_block_outline(language, value));
    }
}

#[test]
fn a_fence_closes_only_on_its_own_character_at_least_as_long() {
    for (source, value) in [
        ("~~~\n```\n~~~\n", "```"),
        ("~~~~\nx\n~~~\n~~~~\n", "x\n~~~"),
        ("```\nx\n~~~\n", "x\n~~~"),
    ] {
        check(source, &code_block_outline(None, value));
    }
}

/// The outline of a document holding only a code block, with no args.
fn code_block_outline(language: Option<&str>, value: &str) -> String {
    let label = language.map_or_else(String::new, |word| format!(" language={word:?}"));
    format!(
        "\
Document
  MarkdownBlock
    Section
      CodeBlock{label} value={value:?}
"
    )
}

#[test]
fn args_that_read_as_the_inside_of_a_toml_inline_table_are_toml() {
    check(
        "```md title=\"README.md\",showLineNumbers=true\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Toml {title = "README.md", showLineNumbers = true} value="x" claimed
"#,
    );
}

#[test]
fn args_with_a_key_and_no_value_are_text() {
    check(
        "```md title=\"README.md\" showLineNumbers\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "title=\"README.md\" showLineNumbers" value="x" claimed
"#,
    );
}

#[test]
fn a_word_after_the_language_is_text_args() {
    check(
        "```Markdown title\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="Markdown" args=Text "title" value="x" claimed
"#,
    );
}

#[test]
fn words_after_the_language_are_text_args() {
    check(
        "```markdown Example of a list\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="markdown" args=Text "Example of a list" value="x" claimed
"#,
    );
}

#[test]
fn args_with_a_key_written_twice_are_text() {
    check(
        "```md a=1,a=2\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "a=1,a=2" value="x" claimed
"#,
    );
}

#[test]
fn args_with_dotted_keys_that_share_a_prefix_are_toml() {
    check(
        "```md a.b=1,a.c=2\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Toml {a = { b = 1, c = 2 }} value="x" claimed
"#,
    );
}

#[test]
fn args_with_a_table_written_twice_are_text() {
    check(
        "```md a={b=1},a={c=2}\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "a={b=1},a={c=2}" value="x" claimed
"#,
    );
}

#[test]
fn args_with_a_dotted_key_through_an_inline_table_are_text() {
    check(
        "```md a={b=1},a.c=2\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "a={b=1},a.c=2" value="x" claimed
"#,
    );
}

#[test]
fn args_with_a_dotted_key_written_twice_are_text() {
    check(
        "```md a.b=1,a.b=2\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "a.b=1,a.b=2" value="x" claimed
"#,
    );
}

#[test]
fn args_whose_inline_table_ends_before_the_last_brace_are_text() {
    check(
        "```md a=1} # x\nx\n```\n",
        r#"Document
  MarkdownBlock
    Section
      CodeBlock language="md" args=Text "a=1} # x" value="x" claimed
"#,
    );
}

#[test]
fn args_nested_past_the_value_cap_are_text_and_written_back_as_they_were() {
    on_two_megabytes(|| {
        // The table the args are read in opens one value around them.
        let lists = format!("a = {}1{}", "[".repeat(VALUE_CAP), "]".repeat(VALUE_CAP));
        let tables = format!(
            "a = {}1{}, c = 2",
            "{b = ".repeat(VALUE_CAP),
            "}".repeat(VALUE_CAP)
        );
        for args in [lists, tables] {
            let source = format!("```md {args}\nx\n```\n");
            let document = parse(&source).document;
            let read = match &valid_body(&document).unwrap().sections[0].blocks[0] {
                Block::CodeBlock(code) => Some(&code.args),
                Block::BlockQuote(_)
                | Block::Html(_)
                | Block::List(_)
                | Block::Paragraph(_)
                | Block::Table(_)
                | Block::ThematicBreak(_) => None,
            };
            assert!(matches!(read, Some(Args::Text(text)) if *text == args));
            assert_eq!(write(&document, &WriteOptions::default()).unwrap(), source);
        }
    });
}

#[test]
fn a_fence_with_no_info_string_has_no_language_and_empty_toml_args() {
    let source = "```\nx\n```\n";
    check(source, &code_block_outline(None, "x"));
    let document = valid(source);
    let read = match &document.body.sections[0].blocks[0] {
        Block::CodeBlock(code) => Some((code.language.as_deref(), &code.args)),
        Block::BlockQuote(_)
        | Block::Html(_)
        | Block::List(_)
        | Block::Paragraph(_)
        | Block::Table(_)
        | Block::ThematicBreak(_) => None,
    };
    assert!(matches!(read, Some((None, Args::Toml(mapping))) if mapping.is_empty()));
}

#[test]
fn toml_args_in_a_grid_cell_read_as_after_a_fence_at_the_root() {
    let fence = "```md title=\"README.md\",showLineNumbers=true";
    let width = fence.len();
    let border = format!("+{}+", "-".repeat(width + 2));
    let lines: Vec<String> = iter::once(border.clone())
        .chain(
            [fence, "x", "```"]
                .iter()
                .map(|line| format!("| {line:<width$} |")),
        )
        .chain(iter::once(border))
        .collect();
    let grid = format!("{}\n", lines.join("\n"));
    for source in [format!("{fence}\nx\n```\n"), grid] {
        let document = valid(&source);
        let mut rows = Rows::default();
        rows.mapping(0, toml_args(&document.body).unwrap());
        assert_eq!(
            outline(&rows.rows),
            r#"Mapping
  Pair key=title
    Scalar String "README.md"
  Pair key=showLineNumbers
    Scalar Boolean true
"#,
            "{source}"
        );
    }
}

/// The `Toml` args of the first code block of `body`, or of the first cell
/// of its first table.
fn toml_args(body: &MarkdownBlock) -> Option<&Mapping> {
    body.sections
        .first()?
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::CodeBlock(code) => match &code.args {
                Args::Text(_) => None,
                Args::Toml(mapping) => Some(mapping),
            },
            Block::Table(table) => {
                let cell = table.head.iter().chain(&table.rows).next()?.cells.first()?;
                toml_args(&cell.body)
            }
            Block::BlockQuote(_)
            | Block::Html(_)
            | Block::List(_)
            | Block::Paragraph(_)
            | Block::ThematicBreak(_) => None,
        })
}

#[test]
fn a_fence_of_tildes_inside_an_item_is_a_code_block_as_one_of_backticks_is() {
    for fence in ["```", "~~~"] {
        check(
            &format!("- step\n\n  {fence}text\n  code\n  {fence}\n- next\n"),
            r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "step"
              CodeBlock language="text" value="code"
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "next"
"#,
        );
    }
}

#[test]
fn a_comment_block_written_by_remargin_is_a_code_block() {
    check(
        "\
Intro.

```remargin
---
id: abc
author: ada
type: human
---
Looks good.
```

More text.
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Intro."
      CodeBlock language="remargin" value="---\nid: abc\nauthor: ada\ntype: human\n---\nLooks good."
      Paragraph
        Line
          Text "More text."
"#,
    );
}

#[test]
fn a_markdown_block_inside_a_nested_item_keeps_its_lines_from_the_fence_column() {
    check(
        "- - ```markdown\n    # A\n    ```\n",
        r##"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              List
                Item
                  MarkdownBlock
                    Section
                      CodeBlock language="markdown" value="# A" claimed
"##,
    );
}

#[test]
fn an_empty_item_does_not_interrupt_a_paragraph() {
    for marker in ["*", "1."] {
        check(
            &format!("text\n{marker}\nmore\n"),
            &format!(
                r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "text"
        Line
          Text "{marker}"
        Line
          Text "more"
"#
            ),
        );
    }
}

#[test]
fn an_empty_item_ends_at_a_blank_line_after_its_marker() {
    check(
        "-\n\n  foo\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
      Paragraph
        Line
          Text "foo"
"#,
    );
    check(
        "1.\n\n   foo\n",
        r#"Document
  MarkdownBlock
    Section
      List start=1
        Item
          MarkdownBlock
      Paragraph
        Line
          Text "foo"
"#,
    );
    check(
        "- -\n\n  foo\n",
        r#"Document
  MarkdownBlock
    Section
      List loose
        Item
          MarkdownBlock
            Section
              List
                Item
                  MarkdownBlock
              Paragraph
                Line
                  Text "foo"
"#,
    );
}

#[test]
fn an_empty_item_holds_the_text_on_the_line_right_after_its_marker() {
    check(
        "-\n  foo\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "foo"
"#,
    );
}

#[test]
fn a_block_quote_holds_its_lines_as_text_without_their_marks() {
    check(
        "\
> # not a heading
> - not a list
>no space
>
>  two spaces
",
        r##"Document
  MarkdownBlock
    Section
      BlockQuote "# not a heading\n- not a list\nno space\n\n two spaces"
"##,
    );
}

#[test]
fn a_line_without_a_mark_ends_a_block_quote() {
    check(
        "\
> a
b

> c

> d
",
        r#"Document
  MarkdownBlock
    Section
      BlockQuote "a"
      Paragraph
        Line
          Text "b"
      BlockQuote "c"
      BlockQuote "d"
"#,
    );
}

#[test]
fn three_or_more_dashes_stars_or_underscores_are_a_thematic_break() {
    check(
        "\
text

---

***

___

- - -

* * *

  _ _ _ _
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "text"
      ThematicBreak
      ThematicBreak
      ThematicBreak
      ThematicBreak
      ThematicBreak
      ThematicBreak
"#,
    );
}

#[test]
fn dashes_on_the_first_line_open_yaml_frontmatter_and_not_a_break() {
    check(
        "---\na: 1\n---\n",
        "\
Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn an_underlined_line_is_a_heading_that_opens_a_section() {
    check(
        "\
Title
=====
text

Sub
---
more

# Next
",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "Title"
      Paragraph
        Line
          Text "text"
      Section
        Heading
          Text "Sub"
        Paragraph
          Line
            Text "more"
    Section
      Heading
        Text "Next"
"#,
    );
}

#[test]
fn the_lines_of_an_underlined_heading_join_with_one_space() {
    check(
        "first\n  second `code`\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "first second "
        Code "code"
"#,
    );
}

#[test]
fn only_equals_signs_and_dashes_underline_paragraph_text() {
    check(
        "\
T
***

text
-
more

a
- - -

b
= =
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "T"
      ThematicBreak
      Section
        Heading
          Text "text"
        Paragraph
          Line
            Text "more"
        Paragraph
          Line
            Text "a"
        ThematicBreak
        Paragraph
          Line
            Text "b"
          Line
            Text "= ="
"#,
    );
}

#[test]
fn an_html_block_holds_its_lines_as_written() {
    check(
        "<details>\n<summary>x</summary>\n</details>\n",
        r#"Document
  MarkdownBlock
    Section
      Html "<details>\n<summary>x</summary>\n</details>"
"#,
    );
}

#[test]
fn each_kind_of_html_block_ends_where_commonmark_ends_it() {
    check(
        r#"<pre>

*a*
</pre> after
text

<!-- a

b --> c
<?php

?>
<!DOCTYPE html>
<![CDATA[
x
]]>
<div class="a">
# not a heading

<a href="x">
y
"#,
        r#"Document
  MarkdownBlock
    Section
      Html "<pre>\n\n*a*\n</pre> after"
      Paragraph
        Line
          Text "text"
      Html "<!-- a\n\nb --> c"
      Html "<?php\n\n?>"
      Html "<!DOCTYPE html>"
      Html "<![CDATA[\nx\n]]>"
      Html "<div class=\"a\">\n# not a heading"
      Html "<a href=\"x\">\ny"
"#,
    );
}

#[test]
fn a_lone_tag_of_any_name_does_not_interrupt_a_paragraph() {
    check(
        "\
a
<div>
b

c
<span>
d
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a"
      Html "<div>\nb"
      Paragraph
        Line
          Text "c"
        Line
          Html "<span>"
        Line
          Text "d"
"#,
    );
}

#[test]
fn a_line_holding_one_whole_tag_starts_an_html_block() {
    check(
        "\
<a b='c' d=e f>

</a >

<a/ >

<a b=c=d>
",
        r#"Document
  MarkdownBlock
    Section
      Html "<a b='c' d=e f>"
      Html "</a >"
      Paragraph
        Line
          Text "<a/ >"
      Paragraph
        Line
          Text "<a b=c=d>"
"#,
    );
}

#[test]
fn a_link_definition_gives_no_node_and_the_parser_keeps_its_target() {
    let source = "\
[1]: https://example.com/guide
[Guide  Two]: <https://example.com/guide> \"Title\"
[guide two]: /later
[a]:
/next-line
  'title'
";
    check(source, "Document\n  MarkdownBlock\n");
    let (_, definitions, _) = block::parse::<Markdown>(source, 0);
    let mut found: Vec<(&str, &str, Option<&str>)> = definitions
        .iter()
        .map(|(label, target)| {
            (
                label.as_str(),
                target.destination.as_str(),
                target.title.as_deref(),
            )
        })
        .collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            ("1", "https://example.com/guide", None),
            ("a", "/next-line", Some("title")),
            ("guide two", "https://example.com/guide", Some("Title")),
        ]
    );
}

#[test]
fn a_definition_does_not_interrupt_a_paragraph_and_nothing_may_follow_it() {
    let source = "\
text
[a]: /u

[b]: /u \"t\" more

[c]: /u
\"t\" more
";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "text"
        Line
          Text "[a]: /u"
      Paragraph
        Line
          Text "[b]: /u \"t\" more"
      Paragraph
        Line
          Text "\"t\" more"
"#,
    );
    let (_, definitions, _) = block::parse::<Markdown>(source, 0);
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions
            .get("c")
            .map(|target| target.destination.as_str()),
        Some("/u")
    );
}

#[test]
fn inside_a_list_item_each_construct_counts_columns_from_the_item_text() {
    let source = "\
- > q
  > r
- a
  ---
- <div>
  x
  </div>
- [x]: /u
-    ***
";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              BlockQuote "q\nr"
        Item
          MarkdownBlock
            Section
              Section
                Heading
                  Text "a"
        Item
          MarkdownBlock
            Section
              Html "<div>\nx\n</div>"
        Item
          MarkdownBlock
        Item
          MarkdownBlock
            Section
              ThematicBreak
"#,
    );
    let (_, definitions, _) = block::parse::<Markdown>(source, 0);
    assert_eq!(
        definitions
            .get("x")
            .map(|target| target.destination.as_str()),
        Some("/u")
    );
}

#[test]
fn a_quoted_line_left_of_the_item_text_ends_the_item() {
    check(
        "- > q\n> r\n",
        r#"Document
  MarkdownBlock
    Section
      List
        Item
          MarkdownBlock
            Section
              BlockQuote "q"
      BlockQuote "r"
"#,
    );
}

#[test]
fn a_block_quote_an_underlined_heading_a_break_and_html_read_together() {
    check(
        "> # quoted\n\nTitle\n=====\n\n***\n\n<div>\nraw\n</div>\n",
        r##"Document
  MarkdownBlock
    Section
      BlockQuote "# quoted"
    Section
      Heading
        Text "Title"
      ThematicBreak
      Html "<div>\nraw\n</div>"
"##,
    );
}

#[test]
fn a_no_break_space_at_a_line_end_is_text_and_only_spaces_and_tabs_make_a_line_blank() {
    check(
        "a\u{a0}\nb\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a\u{a0}"
        Line
          Text "b"
"#,
    );
    check(
        "a\n\u{a0}\nb\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a"
        Line
          Text "\u{a0}"
        Line
          Text "b"
"#,
    );
    check(
        "a\n \t\nb\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a"
      Paragraph
        Line
          Text "b"
"#,
    );
}

#[test]
fn a_heading_keeps_a_no_break_space_at_its_end() {
    check(
        "# a\u{a0}\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "a\u{a0}"
"#,
    );
}

#[test]
fn a_definition_title_over_a_line_end_keeps_the_spaces_before_it() {
    check(
        "[x]: /u \"t  \nx\"\n\n[a][x]\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="/u" title="t  \nx"
            Text "a"
"#,
    );
    check(
        "[x]: /u  \n\"t\"\n\n[a][x]\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="/u" title="t"
            Text "a"
"#,
    );
}

/// How deep lists of one item nest from `body` down, and the body of the
/// innermost item.
fn list_depth(body: &MarkdownBlock) -> (usize, &MarkdownBlock) {
    let mut inner = body;
    let mut depth = 0_usize;
    while let [section] = inner.sections.as_slice()
        && let [Block::List(list)] = section.blocks.as_slice()
        && let [item] = list.items.as_slice()
    {
        depth += 1;
        inner = &item.body;
    }
    (depth, inner)
}

/// Checks that `document` holds lists nested `BLOCK_CAP` deep, the
/// innermost item holding one line of the text `rest`.
fn assert_block_capped(document: &RawDocument, rest: &str) {
    let (depth, innermost) = list_depth(valid_body(document).unwrap());
    assert_eq!(depth, BLOCK_CAP);
    let lines = match innermost.sections.as_slice() {
        [section] => match section.blocks.as_slice() {
            [Block::Paragraph(paragraph)] => paragraph.lines.as_slice(),
            _ => &[],
        },
        _ => &[],
    };
    assert!(matches!(lines, [line]
        if matches!(line.children.as_slice(), [Inline::Text(text)] if text.value == rest)));
}

#[test]
fn lists_nest_the_block_cap_deep_and_the_markers_past_it_stay_text() {
    on_two_megabytes(|| {
        let document = parse(&format!("{}a\n", "- ".repeat(10_000))).document;
        let rest = format!("{}a", "- ".repeat(10_000 - BLOCK_CAP));
        assert_block_capped(&document, &rest);
        let written = write(&document, &WriteOptions::default()).unwrap();
        assert_eq!(written, format!("{}\\{rest}\n", "- ".repeat(BLOCK_CAP)));
        assert_block_capped(&parse(&written).document, &rest);
        let copy = document.clone();
        assert_eq!(copy, document);
        let printed = format!("{document:?}");
        assert_eq!(printed.matches("List(").count(), BLOCK_CAP);
        drop(copy);
        drop(document);
    });
}

#[test]
fn lists_at_the_block_cap_read_back_from_their_written_text_to_the_same_tree() {
    on_two_megabytes(|| {
        let source = format!("{}a\n", "- ".repeat(BLOCK_CAP));
        let document = parse(&source).document;
        assert_eq!(list_depth(valid_body(&document).unwrap()).0, BLOCK_CAP);
        let written = write(&document, &WriteOptions::default()).unwrap();
        assert_eq!(written, source);
        assert_eq!(parse(&written).document, document);
    });
}

#[test]
fn an_item_past_the_block_cap_on_its_own_line_continues_the_innermost_paragraph() {
    on_two_megabytes(|| {
        let mut source = String::new();
        for depth in 0..=BLOCK_CAP {
            source.push_str(&"  ".repeat(depth));
            source.push_str("- a\n");
        }
        let document = valid(&source);
        let mut blocks = document.body.sections[0].blocks.as_slice();
        let mut depth = 0_usize;
        while let Some(Block::List(list)) = blocks.last()
            && let [item] = list.items.as_slice()
            && let [section] = item.body.sections.as_slice()
        {
            depth += 1;
            blocks = &section.blocks;
        }
        assert_eq!(depth, BLOCK_CAP);
        let lines = match blocks {
            [Block::Paragraph(paragraph)] => paragraph.lines.as_slice(),
            _ => &[],
        };
        let values: Vec<&str> = lines
            .iter()
            .map(|line| match line.children.as_slice() {
                [Inline::Text(text)] => text.value.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(values, ["a", "- a"]);
    });
}

/// The body of the one container `body` holds, when it holds nothing else: a
/// list of one item, a claimed markdown block, or a table of one cell.
fn only_container(body: &MarkdownBlock) -> Option<&MarkdownBlock> {
    let [section] = body.sections.as_slice() else {
        return None;
    };
    match section.blocks.as_slice() {
        [Block::CodeBlock(code)] => code.kind.0.as_deref().and_then(valid_body),
        [Block::List(list)] => match list.items.as_slice() {
            [item] => Some(&item.body),
            _ => None,
        },
        [Block::Table(table)] => match (table.head.as_slice(), table.rows.as_slice()) {
            ([], [row]) => match row.cells.as_slice() {
                [cell] => Some(&cell.body),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// How many containers nest from `body` down, each the only block of the one
/// before, and the body of the innermost.
fn innermost(body: &MarkdownBlock) -> (usize, &MarkdownBlock) {
    let mut inner = body;
    let mut depth = 0_usize;
    while let Some(next) = only_container(inner) {
        depth += 1;
        inner = next;
    }
    (depth, inner)
}

/// How deep list items, claimed markdown blocks and table cells nest
/// anywhere in `body`.
fn container_depth(body: &MarkdownBlock) -> usize {
    let mut deepest = 0;
    let mut bodies = vec![(body, 0_usize)];
    while let Some((block, depth)) = bodies.pop() {
        deepest = deepest.max(depth);
        let mut sections: Vec<&Section> = block.sections.iter().collect();
        while let Some(section) = sections.pop() {
            sections.extend(&section.subsections);
            for each in &section.blocks {
                match each {
                    Block::CodeBlock(code) => {
                        let inner = code.kind.0.as_deref().and_then(valid_body);
                        bodies.extend(inner.map(|claimed| (claimed, depth + 1)));
                    }
                    Block::List(list) => {
                        bodies.extend(list.items.iter().map(|item| (&item.body, depth + 1)));
                    }
                    Block::Table(table) => bodies.extend(
                        table
                            .head
                            .iter()
                            .chain(&table.rows)
                            .flat_map(|row| &row.cells)
                            .map(|cell| (&cell.body, depth + 1)),
                    ),
                    Block::BlockQuote(_)
                    | Block::Html(_)
                    | Block::Paragraph(_)
                    | Block::ThematicBreak(_) => {}
                }
            }
        }
    }
    deepest
}

/// `levels` markdown blocks around `a`, each inside the one before: the
/// innermost fence is three backticks, and each one around it one longer.
pub(super) fn nested_markdown_blocks(levels: usize) -> String {
    let fences: Vec<String> = (3..levels + 3).map(|length| "`".repeat(length)).collect();
    let mut source = String::new();
    for fence in fences.iter().rev() {
        source.push_str(fence);
        source.push_str("markdown\n");
    }
    source.push_str("a\n");
    for fence in &fences {
        source.push_str(fence);
        source.push('\n');
    }
    source
}

/// `levels` grid tables around `a`, each in the single cell of the one
/// before.
pub fn nested_grid_tables(levels: usize) -> String {
    let line =
        |depth: usize, text: &str| format!("{}{text}{}\n", "| ".repeat(depth), " |".repeat(depth));
    let borders: Vec<String> = (0..levels)
        .map(|depth| {
            line(
                depth,
                &format!("+{}+", "-".repeat(4 * (levels - depth) - 1)),
            )
        })
        .collect();
    let mut source = borders.concat();
    source.push_str(&line(levels, "a"));
    for border in borders.iter().rev() {
        source.push_str(border);
    }
    source
}

/// `levels` containers around `a`, each inside the one before: a list item,
/// a markdown block and a grid table in turn, from the outside in.
fn mixed_containers(levels: usize) -> String {
    let mut lines = vec!["a".to_owned()];
    let mut fence = 2_usize;
    for level in (0..levels).rev() {
        lines = match level % 3 {
            0 => lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    if index == 0 {
                        format!("- {line}")
                    } else {
                        format!("  {line}")
                    }
                })
                .collect(),
            1 => {
                fence += 1;
                let mark = "`".repeat(fence);
                iter::once(format!("{mark}markdown"))
                    .chain(lines)
                    .chain(iter::once(mark))
                    .collect()
            }
            _ => {
                let width = lines.iter().map(String::len).max().unwrap_or_default();
                let border = format!("+{}+", "-".repeat(width + 2));
                iter::once(border.clone())
                    .chain(lines.iter().map(|line| format!("| {line:<width$} |")))
                    .chain(iter::once(border))
                    .collect()
            }
        };
    }
    let mut source = lines.join("\n");
    source.push('\n');
    source
}

#[test]
fn markdown_blocks_nest_the_block_cap_deep_and_the_block_past_it_stays_a_code_block() {
    on_two_megabytes(|| {
        let source = nested_markdown_blocks(1000);
        let document = parse(&source).document;
        let (depth, inner) = innermost(valid_body(&document).unwrap());
        assert_eq!(depth, BLOCK_CAP);
        let rest = nested_markdown_blocks(1000 - BLOCK_CAP - 1);
        let plain = match inner.sections.as_slice() {
            [section] => match section.blocks.as_slice() {
                [Block::CodeBlock(code)] => Some(code),
                _ => None,
            },
            _ => None,
        };
        assert!(plain.is_some_and(|code| code.kind.0.is_none()
            && code.language.as_deref() == Some("markdown")
            && rest.strip_suffix('\n') == Some(code.value.as_str())));
        let written = write(&document, &WriteOptions::default()).unwrap();
        assert_eq!(written, source);
        let copy = document.clone();
        assert_eq!(copy, document);
        let printed = format!("{document:?}");
        assert_eq!(printed.matches("Kind(Some(").count(), BLOCK_CAP);
        drop(copy);
        drop(document);
    });
}

#[test]
fn grid_tables_nest_the_block_cap_deep_and_the_tables_past_it_stay_text() {
    on_two_megabytes(|| {
        let source = nested_grid_tables(1000);
        let document = parse(&source).document;
        let (depth, inner) = innermost(valid_body(&document).unwrap());
        assert_eq!(depth, BLOCK_CAP);
        let lines = match inner.sections.as_slice() {
            [section] => match section.blocks.as_slice() {
                [Block::Paragraph(paragraph)] => paragraph.lines.as_slice(),
                _ => &[],
            },
            _ => &[],
        };
        let texts: Vec<&str> = lines
            .iter()
            .map(|line| match line.children.as_slice() {
                [Inline::Text(text)] => text.value.as_str(),
                _ => "",
            })
            .collect();
        let rest = nested_grid_tables(1000 - BLOCK_CAP);
        assert_eq!(texts, rest.lines().collect::<Vec<_>>());
        let written = write(&document, &WriteOptions::default()).unwrap();
        assert_eq!(written.lines().count(), source.lines().count());
        let copy = document.clone();
        assert_eq!(copy, document);
        let printed = format!("{document:?}");
        assert_eq!(printed.matches("Table {").count(), BLOCK_CAP);
        drop(copy);
        drop(document);
    });
}

#[test]
fn lists_markdown_blocks_and_grid_tables_in_turn_nest_the_block_cap_deep_together() {
    on_two_megabytes(|| {
        let document = valid(&mixed_containers(1000));
        assert_eq!(innermost(&document.body).0, BLOCK_CAP);
        assert_eq!(container_depth(&document.body), BLOCK_CAP);
    });
}

/// `BLOCK_CAP` containers around emphasis `INLINE_CAP` deep, each inside the
/// one before and each holding the headings `# h` to `###### h` before it:
/// those of `cycle` in turn, from the outside in.
fn headed_containers(cycle: &[Container]) -> String {
    let marks = "*".repeat(2 * INLINE_CAP);
    let mut lines = vec![format!("{marks}a{marks}")];
    let mut fence = 2_usize;
    for level in (0..BLOCK_CAP).rev() {
        let headed = (1..=6)
            .map(|count| format!("{} h", "#".repeat(count)))
            .chain(lines);
        lines = match cycle[level % cycle.len()] {
            Container::Grid => {
                let rows: Vec<String> = headed.collect();
                let width = rows.iter().map(String::len).max().unwrap_or_default();
                let border = format!("+{}+", "-".repeat(width + 2));
                iter::once(border.clone())
                    .chain(rows.iter().map(|line| format!("| {line:<width$} |")))
                    .chain(iter::once(border))
                    .collect()
            }
            Container::List => headed
                .enumerate()
                .map(|(index, line)| {
                    if index == 0 {
                        format!("- {line}")
                    } else {
                        format!("  {line}")
                    }
                })
                .collect(),
            Container::Markdown => {
                fence += 1;
                let mark = "`".repeat(fence);
                iter::once(format!("{mark}markdown"))
                    .chain(headed)
                    .chain(iter::once(mark))
                    .collect()
            }
        };
    }
    let mut source = lines.join("\n");
    source.push('\n');
    source
}

#[test]
fn containers_that_hold_headings_of_every_level_nest_the_block_cap_deep() {
    on_two_megabytes(|| {
        let shapes: [&[Container]; 4] = [
            &[Container::List],
            &[Container::Markdown],
            &[Container::Grid],
            &[Container::List, Container::Markdown, Container::Grid],
        ];
        for cycle in shapes {
            let document = parse(&headed_containers(cycle)).document;
            assert_eq!(container_depth(valid_body(&document).unwrap()), BLOCK_CAP);
            let written = write(&document, &WriteOptions::default()).unwrap();
            assert_eq!(written.matches("###### h").count(), BLOCK_CAP);
            let copy = document.clone();
            assert_eq!(copy, document);
            let printed = format!("{document:?}");
            assert_eq!(printed.matches("Section {").count(), 1 + 6 * BLOCK_CAP);
            drop(copy);
            let json = to_json(&document).unwrap();
            let back: RawDocument = from_json(&json).unwrap();
            assert_eq!(to_json(&back).unwrap(), json);
            drop(back);
            drop(document);
        }
    });
}
