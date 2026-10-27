//! Whole documents. Each is a file under `fixtures/`, with the outline of
//! the tree it parses into beside it in a `.tree` file, or a source written
//! here. Each reads back from JSON as that tree, every `kind` empty, and its
//! extension nodes build again from it. No fixture's frontmatter drops an
//! entry.

use super::json::{reads_back_from_json, rebuilds_from_json};
use super::{check, check_spans};
use crate::parser::ast::RawDocument;
use crate::parser::parse;

macro_rules! document {
    ($name:ident) => {
        #[test]
        fn $name() {
            let source = include_str!(concat!("fixtures/", stringify!($name), ".md"));
            check(
                source,
                include_str!(concat!("fixtures/", stringify!($name), ".tree")),
            );
            let parsed = parse(source);
            assert!(matches!(parsed.document, RawDocument::Valid(_)));
            assert!(parsed.dropped.is_empty());
            reads_back_from_json(source);
            rebuilds_from_json(source);
        }
    };
}

// TOML frontmatter.
document!(hugo_post);
document!(hugo_post_with_table);
document!(task);
document!(toml_multi_line_string);
document!(zola_page);

// YAML frontmatter.
document!(contact);
document!(docusaurus_page);
document!(issue_template);
document!(jekyll_post);
document!(meeting_notes);
document!(obsidian_note);
document!(pandoc_paper);
document!(recipe);
document!(slash_command);
document!(yaml_list_of_mappings);

// No frontmatter.
document!(api_reference);
document!(book_page);
document!(changelog);
document!(daily_note);
document!(essay);
document!(readme);

#[test]
fn a_heading_after_a_byte_order_mark_opens_the_first_section() {
    let source = "\u{feff}# A\n\ntext\n";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "A"
      Paragraph
        Line
          Text "text"
"#,
    );
    check_spans(
        source,
        &[
            ("Document", source),
            ("MarkdownBlock", "# A\n\ntext\n"),
            ("Section", "# A\n\ntext"),
            ("Heading", "# A"),
            (r#"Text "A""#, "A"),
            ("Paragraph", "text"),
            ("Line", "text"),
            (r#"Text "text""#, "text"),
        ],
    );
    reads_back_from_json(source);
    rebuilds_from_json(source);
}

#[test]
fn a_byte_order_mark_after_the_first_byte_is_text() {
    let source = "a\u{feff}b\n";
    check(
        source,
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a\u{feff}b"
"#,
    );
    reads_back_from_json(source);
    rebuilds_from_json(source);
}
