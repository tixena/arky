use core::slice;

use super::{broken, check, check_spans, located_frontmatter, on_two_megabytes};
use crate::parser::ast::{
    FrontmatterFormat, InvalidDocument, InvalidFrontmatter, Mapping, RawDocument, Value,
};
use crate::parser::error::ParseError;
use crate::parser::extension::Plain;
use crate::parser::frontmatter::VALUE_CAP;
use crate::parser::located::BrokenFrontmatter;
use crate::parser::{ParseOptions, parse, parse_with, yaml};
use crate::writer::{WriteOptions, write};

/// The outline of a YAML frontmatter that holds no pair, before an empty body.
const EMPTY_YAML: &str = "Document
  Frontmatter format=Yaml
    Mapping
  MarkdownBlock
";

/// The outline of a YAML frontmatter whose every pair is dropped for a key
/// that is a list or a mapping, before an empty body.
const LIST_OR_MAPPING_KEY: &str = r#"InvalidDocument
  InvalidFrontmatter format=Yaml error="a key cannot be a list or a mapping"
  MarkdownBlock
"#;

/// The outline of a YAML frontmatter that holds no pair, before the heading
/// `A`.
const EMPTY_BEFORE_A: &str = r#"Document
  Frontmatter format=Yaml
    Mapping
  MarkdownBlock
    Section
      Heading
        Text "A"
"#;

#[test]
fn a_file_with_no_delimiter_on_its_first_line_has_no_frontmatter() {
    check(
        "Just text.\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Just text."
"#,
    );
}

#[test]
fn an_opening_delimiter_with_no_closing_one_is_a_thematic_break() {
    check(
        "\
---
title: x

No closing line.
",
        r#"Document
  MarkdownBlock
    Section
      ThematicBreak
      Paragraph
        Line
          Text "title: x"
      Paragraph
        Line
          Text "No closing line."
"#,
    );
}

#[test]
fn an_empty_frontmatter_is_a_mapping_with_no_pairs() {
    check(
        "\
+++
+++
Body.
",
        r#"Document
  Frontmatter format=Toml
    Mapping
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Body."
"#,
    );
}

#[test]
fn a_delimiter_after_the_first_line_is_a_thematic_break() {
    check(
        "\
Text.

---

More.
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Text."
      ThematicBreak
      Paragraph
        Line
          Text "More."
"#,
    );
}

#[test]
fn three_dots_also_close_a_yaml_frontmatter() {
    check(
        "\
---
a: 1
...
",
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
fn windows_line_endings_read_the_same() {
    check(
        "---\r\ntitle: x\r\n---\r\n# H\r\n\r\n- a\r\n  b\r\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "x"
  MarkdownBlock
    Section
      Heading
        Text "H"
      List
        Item
          MarkdownBlock
            Section
              Paragraph
                Line
                  Text "a"
                Line
                  Text "b"
"#,
    );
}

#[test]
fn a_frontmatter_may_end_the_file_with_no_line_ending() {
    check(
        "---\na: 1\n---",
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
fn spaces_may_follow_a_delimiter_but_not_come_before_it() {
    check(
        "---  \na: 1\n---  \n",
        "\
Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        " ---\na: 1\n---\n",
        r#"Document
  MarkdownBlock
    Section
      ThematicBreak
      Section
        Heading
          Text "a: 1"
"#,
    );
}

#[test]
fn a_yaml_frontmatter_after_a_byte_order_mark_is_read() {
    let source = "\u{feff}---\na: 1\n---\n# A\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
    Section
      Heading
        Text "A"
"#,
    );
    check_spans(
        source,
        &[
            ("Document", source),
            ("Frontmatter format=Yaml", "---\na: 1\n---"),
            ("Mapping", "a: 1"),
            ("Pair key=a", "a: 1"),
            ("Scalar Integer 1", "1"),
            ("MarkdownBlock", "# A\n"),
            ("Section", "# A"),
            ("Heading", "# A"),
            (r#"Text "A""#, "A"),
        ],
    );
}

#[test]
fn a_toml_frontmatter_after_a_byte_order_mark_is_read() {
    let source = "\u{feff}+++\na = 1\n+++\nx\n";
    check(
        source,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "x"
"#,
    );
    check_spans(
        source,
        &[
            ("Document", source),
            ("Frontmatter format=Toml", "+++\na = 1\n+++"),
            ("Mapping", "a = 1"),
            ("Pair key=a", "a = 1"),
            ("Scalar Integer 1", "1"),
            ("MarkdownBlock", "x\n"),
            ("Section", "x"),
            ("Paragraph", "x"),
            ("Line", "x"),
            (r#"Text "x""#, "x"),
        ],
    );
}

#[test]
fn a_yaml_pair_whose_key_is_a_list_or_a_mapping_is_dropped_and_the_rest_is_read() {
    check(
        "---\ntitle: Trip\n[48.85, 2.35]: Paris\nyear: 2026\n---\n# Trip\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "Trip"
      Pair key=year
        Scalar Integer 2026
  MarkdownBlock
    Section
      Heading
        Text "Trip"
"#,
    );
    let first_kept = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
";
    check("---\na: 1\n[x]: 2\n---\n", first_kept);
    check("---\na: 1\n{x: 2}: 3\n---\n", first_kept);
    check("---\na: 1\n!!seq [x]: y\n---\n", first_kept);
    check("---\na: 1\n[x]: 2\n[x]: 3\n---\n", first_kept);
    check("---\na: 1\n[x]:\n  b: 2\n---\n", first_kept);
}

#[test]
fn an_explicit_key_that_is_a_list_or_a_mapping_drops_its_pair() {
    check(
        "---\ntitle: Trip\n? - 48.85\n  - 2.35\n: Paris\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "Trip"
  MarkdownBlock
"#,
    );
    let last_kept = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
";
    check("---\n? a: 1\n: v\nb: 2\n---\n", last_kept);
    check("---\n? {x: 1}\n: v\nb: 2\n---\n", last_kept);
    for source in [
        "---\n? [a, b]\n: v\n---\n",
        "---\n?\n  a: 1\n: v\n---\n",
        "---\n?\n  - a\n: v\n---\n",
        "---\n? - a\n: v\n---\n",
        "---\n? a: 1\n: v\n---\n",
        "---\n? !!seq [x]\n: v\n---\n",
        "---\n? [x]\n---\n",
    ] {
        check(source, LIST_OR_MAPPING_KEY);
    }
}

#[test]
fn a_mapping_that_held_only_a_dropped_pair_is_an_empty_mapping() {
    check("---\n{x: 2}: 3\n---\n", LIST_OR_MAPPING_KEY);
    check("---\n[x]: y\n---\n", LIST_OR_MAPPING_KEY);
    check(
        "---\nc:\n  [x]: y\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=c
        Mapping
  MarkdownBlock
",
    );
    check(
        "---\nitems:\n- {x: 1}: 12\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
  MarkdownBlock
",
    );
    check(
        "---\nc:\n  [x]: y\n  d: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=c
        Mapping
          Pair key=d
            Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_flow_pair_whose_key_is_a_list_or_a_mapping_is_dropped() {
    let mapping = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=b
            Scalar Integer 2
  MarkdownBlock
";
    check("---\na: {[x]: 1, b: 2}\n---\n", mapping);
    check("---\na: {? [x] : 1, b: 2}\n---\n", mapping);
    check("---\na: {!!map {x: 1}: 1, b: 2}\n---\n", mapping);
    check("---\na: {[x], b: 2}\n---\n", mapping);
    check(
        "---\na: [[x]: 1, c]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
          Scalar String "c"
  MarkdownBlock
"#,
    );
    let one_empty = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
  MarkdownBlock
";
    check("---\na: [? [x] : 1]\n---\n", one_empty);
    check("---\na: [{x: 1}:2]\n---\n", one_empty);
    check("---\na: [!!seq [x] : 1]\n---\n", one_empty);
    check(
        "---\nowner: {[a]: Ada}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
  MarkdownBlock
",
    );
    check("---\n{[x]: 1}\n---\n", LIST_OR_MAPPING_KEY);
}

#[test]
fn a_mapping_whose_every_pair_is_dropped_is_empty_at_its_first_pair() {
    let nested = "---\nc:\n  [x]: y\n---\n";
    check_spans(
        nested,
        &[
            ("Document", nested),
            ("Frontmatter format=Yaml", "---\nc:\n  [x]: y\n---"),
            ("Mapping", "c:\n  "),
            ("Pair key=c", "c:\n  "),
            ("Mapping", ""),
            ("MarkdownBlock", ""),
        ],
    );
    let entry = "---\nitems:\n- {x: 1}: 12\n---\n";
    check_spans(
        entry,
        &[
            ("Document", entry),
            ("Frontmatter format=Yaml", "---\nitems:\n- {x: 1}: 12\n---"),
            ("Mapping", "items:\n- "),
            ("Pair key=items", "items:\n- "),
            ("Sequence", "- "),
            ("Mapping", ""),
            ("MarkdownBlock", ""),
        ],
    );
    let root = "---\n# a comment\n  [x]: y\n---\n";
    let at = root.find('[').unwrap();
    assert_eq!(broken(root).error.span, at..at + "[x]".len());
}

#[test]
fn a_yaml_integer_too_large_for_64_bits_is_the_text_of_its_digits() {
    let source = "---\nn: 99999999999999999999\n---\n";
    check_spans(
        source,
        &[
            ("Document", source),
            (
                "Frontmatter format=Yaml",
                "---\nn: 99999999999999999999\n---",
            ),
            ("Mapping", "n: 99999999999999999999"),
            ("Pair key=n", "n: 99999999999999999999"),
            (
                "Scalar String \"99999999999999999999\"",
                "99999999999999999999",
            ),
            ("MarkdownBlock", ""),
        ],
    );
    check(
        "---\nn: [1, 99999999999999999999]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=n
        Sequence
          Scalar Integer 1
          Scalar String "99999999999999999999"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_yaml_integer_too_large_for_64_bits_is_its_text_as_written_wherever_it_stands() {
    check(
        "\
---
hex: 0xFFFFFFFFFFFFFFFFFF
negative: -99999999999999999999
under:
  99999999999999999999
entries:
  - 99999999999999999999
flow: {n: 99999999999999999999}
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=hex
        Scalar String "0xFFFFFFFFFFFFFFFFFF"
      Pair key=negative
        Scalar String "-99999999999999999999"
      Pair key=under
        Scalar String "99999999999999999999"
      Pair key=entries
        Sequence
          Scalar String "99999999999999999999"
      Pair key=flow
        Mapping
          Pair key=n
            Scalar String "99999999999999999999"
  MarkdownBlock
"#,
    );
}

#[test]
fn digits_too_large_for_64_bits_under_an_int_tag_are_text_and_under_a_float_tag_a_float() {
    check(
        "\
---
int: !!int 99999999999999999999
float: !!float 99999999999999999999
text: !!str 99999999999999999999
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=int
        Scalar String "99999999999999999999"
      Pair key=float
        Scalar Float 100000000000000000000
      Pair key=text
        Scalar String "99999999999999999999"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_toml_integer_too_large_for_64_bits_is_its_text_as_written() {
    check(
        "\
+++
n = 99999999999999999999
negative = -99999999999999999999
underscores = 9_999_999_999_999_999_999
hex = 0xFFFFFFFFFFFFFFFFFF
binary = 0b11111111111111111111111111111111111111111111111111111111111111111
list = [1, 99999999999999999999]
+++
",
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=n
        Scalar String "99999999999999999999"
      Pair key=negative
        Scalar String "-99999999999999999999"
      Pair key=underscores
        Scalar String "9_999_999_999_999_999_999"
      Pair key=hex
        Scalar String "0xFFFFFFFFFFFFFFFFFF"
      Pair key=binary
        Scalar String "0b11111111111111111111111111111111111111111111111111111111111111111"
      Pair key=list
        Sequence
          Scalar Integer 1
          Scalar String "99999999999999999999"
  MarkdownBlock
"#,
    );
}

#[test]
fn an_integer_that_fits_64_bits_stays_an_integer_in_yaml_and_toml() {
    let integers = |format: &str| {
        format!(
            "Document
  Frontmatter format={format}
    Mapping
      Pair key=largest
        Scalar Integer 9223372036854775807
      Pair key=smallest
        Scalar Integer -9223372036854775808
  MarkdownBlock
"
        )
    };
    check(
        "---\nlargest: 9223372036854775807\nsmallest: -9223372036854775808\n---\n",
        &integers("Yaml"),
    );
    check(
        "+++\nlargest = 9223372036854775807\nsmallest = -9223372036854775808\n+++\n",
        &integers("Toml"),
    );
}

/// The outline of a frontmatter in `format` that holds `b: 2` alone, before an
/// empty body.
fn only_b(format: &str) -> String {
    format!(
        "Document
  Frontmatter format={format}
    Mapping
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
"
    )
}

#[test]
fn a_tag_arky_does_not_read_drops_the_innermost_key_that_holds_it() {
    for source in [
        "---\na: !point {x: 1}\nb: 2\n---\n",
        "---\n!point a: 1\nb: 2\n---\n",
        "---\ntags: [!point x, y]\nb: 2\n---\n",
        "---\norigin: !point 3,4\nb: 2\n---\n",
        "---\ndata: !!binary R0lG\nb: 2\n---\n",
        "---\norigin: !<tag:example.com,2026:point> 3,4\nb: 2\n---\n",
        "---\npoints: [!point 3]\nb: 2\n---\n",
        "---\n!point origin: 3\nb: 2\n---\n",
        "---\na: !point\n  x: 1\nb: 2\n---\n",
        "---\na:\n- !point x\n- y\nb: 2\n---\n",
        "---\n? !point k\n: v\nb: 2\n---\n",
        "---\n[!point x]: 1\nb: 2\n---\n",
        "---\na: !point \"x, ]\" # ]\n  [y\nb: 2\n---\n",
    ] {
        check(source, &only_b("Yaml"));
    }
    check(
        "---\nowner:\n  pin: !point 1\n  name: Ada\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_tag_arky_does_not_read_inside_a_flow_collection_drops_the_pair_that_holds_it() {
    let flow_b = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=b
            Scalar Integer 2
  MarkdownBlock
";
    check("---\na: {x: !point 1, b: 2}\n---\n", flow_b);
    check("---\na: {!point x: 1, b: 2}\n---\n", flow_b);
    check("---\na: {x: [!point {y: \"}\"}], b: 2}\n---\n", flow_b);
    check("---\n{x: !point [1, 2],\nb: 2}\n---\n", &only_b("Yaml"));
    let one_pair = r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
          Scalar String "c"
  MarkdownBlock
"#;
    check("---\na: [!point x: 1, c]\n---\n", one_pair);
    check("---\na: [x: !point 1, c]\n---\n", one_pair);
    check("---\na: [[!point x]: 1, c]\n---\n", one_pair);
    check(
        "---\nitems:\n- a: !point 1\n  b: 2\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_tag_that_ends_its_line_over_a_mapping_whose_every_pair_is_dropped_holds_it_empty() {
    let empty_c = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=c
        Mapping
      Pair key=d
        Scalar Integer 1
  MarkdownBlock
";
    check("---\nc: !!map\n  a: !point x\nd: 1\n---\n", empty_c);
    check(
        "---\nc: !!map\n  [x]: y\n\n  # a comment\nd: 1\n---\n",
        empty_c,
    );
}

#[test]
fn a_frontmatter_with_a_dropped_tag_is_written_without_its_key() {
    let document = parse("---\ntitle: Notes\nshape: !point {x: 1, y: 2}\n---\n# Notes\n").document;
    let mapping = frontmatter_of(&document);
    assert_eq!(mapping.keys().collect::<Vec<_>>(), ["title"]);
    assert_eq!(
        write(&document, &WriteOptions::default()).unwrap(),
        "---\ntitle: Notes\n---\n# Notes\n"
    );
}

/// `deep:` and a YAML list nested `depth` deep in each of its forms, beside
/// `b: 2`.
fn deep_yaml_lists(depth: usize) -> [String; 3] {
    let mut lines = String::new();
    for level in 0..depth {
        lines.push_str(&"  ".repeat(level));
        lines.push_str("-\n");
    }
    lines.push_str(&"  ".repeat(depth));
    lines.push_str("1\n");
    let flow = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    [
        format!("---\ndeep:\n{lines}b: 2\n---\n"),
        format!("---\ndeep:\n  {}1\nb: 2\n---\n", "- ".repeat(depth)),
        format!("---\ndeep: {flow}\nb: 2\n---\n"),
    ]
}

#[test]
fn a_yaml_value_nested_past_the_value_cap_drops_the_key_that_holds_it() {
    on_two_megabytes(|| {
        for source in deep_yaml_lists(VALUE_CAP + 1) {
            check(&source, &only_b("Yaml"));
        }
        for source in deep_yaml_lists(VALUE_CAP) {
            let document = parse(&source).document;
            let mapping = frontmatter_of(&document);
            assert_eq!(mapping.keys().collect::<Vec<_>>(), ["deep", "b"]);
        }
    });
}

#[test]
fn a_toml_value_nested_past_the_value_cap_drops_its_pair() {
    on_two_megabytes(|| {
        let past = VALUE_CAP + 1;
        let arrays = format!("{}1{}", "[".repeat(past), "]".repeat(past));
        let table = format!("{}{{x = 1}}{}", "[".repeat(past - 1), "]".repeat(past - 1));
        let quoted = format!(
            "[\n  # ] a comment\n  \"]\", '[', \"\"\"\n]\"\"\", {}1{}]",
            "[".repeat(past),
            "]".repeat(past)
        );
        for value in [arrays, table, quoted] {
            check(
                &format!("+++\ndeep = {value}\nb = 2\n+++\n"),
                &only_b("Toml"),
            );
        }
        let dotted = "a.".repeat(past);
        check(
            &format!("+++\n{dotted}x = 1\nb = 2\n+++\n"),
            &only_b("Toml"),
        );
    });
}

#[test]
fn a_toml_header_nested_past_the_value_cap_drops_its_table_and_its_pairs() {
    let source = format!(
        "+++\nb = 2\n[{}x]\nc = [\n[1],\n]\nd = \"\"\"\n[e]\n\"\"\"\n[t]\nf = 3\n+++\n",
        "a.".repeat(VALUE_CAP)
    );
    check(
        &source,
        "Document
  Frontmatter format=Toml
    Mapping
      Pair key=b
        Scalar Integer 2
      Pair key=t
        Mapping
          Pair key=f
            Scalar Integer 3
  MarkdownBlock
",
    );
}

#[test]
fn a_toml_table_keeps_its_span_to_its_last_pair_that_is_not_dropped() {
    on_two_megabytes(|| {
        let source = format!(
            "+++\n[t]\na = 1\nb = {}1{}\n+++\n",
            "[".repeat(VALUE_CAP + 1),
            "]".repeat(VALUE_CAP + 1)
        );
        let mapping = located_frontmatter(&source);
        let pair = &mapping.pairs["t"];
        assert_eq!(&source[pair.span.clone()], "[t]\na = 1");
    });
}

#[test]
fn a_yaml_frontmatter_that_holds_one_value_or_one_list_is_empty() {
    for source in ["---\nabc\n---\n# A\n", "---\n- a\n- b\n---\n# A\n"] {
        check(source, EMPTY_BEFORE_A);
        let document = parse(source).document;
        assert_eq!(
            write(&document, &WriteOptions::default()).unwrap(),
            "---\n---\n# A\n"
        );
    }
    for content in [
        "[a, b]",
        "hello",
        "\"a\n  b\"",
        "- a",
        "99999999999999999999",
        "- 99999999999999999999",
        "[99999999999999999999]",
        "abc # a comment\n\n# another",
        "  - a\n  - b",
        "!!seq\n- a",
    ] {
        check(&format!("---\n{content}\n---\n# A\n"), EMPTY_BEFORE_A);
    }
}

#[test]
fn a_yaml_frontmatter_that_holds_one_value_or_one_list_with_a_drop_is_empty() {
    on_two_megabytes(|| {
        // A list at the root opens no level of its own.
        let past = VALUE_CAP + 2;
        for content in [
            "!point x".to_owned(),
            "- !point x\n- y".to_owned(),
            "[!point x, y]".to_owned(),
            "!!seq\n- !point x".to_owned(),
            format!("{}1{}", "[".repeat(past), "]".repeat(past)),
            format!("{}1", "- ".repeat(past)),
        ] {
            check(&format!("---\n{content}\n---\n# A\n"), EMPTY_BEFORE_A);
        }
    });
}

#[test]
fn the_empty_mapping_of_a_frontmatter_that_holds_one_value_spans_the_value() {
    let list = "---\n- a\n- b\n---\n";
    check_spans(
        list,
        &[
            ("Document", list),
            ("Frontmatter format=Yaml", "---\n- a\n- b\n---"),
            ("Mapping", "- a\n- b"),
            ("MarkdownBlock", ""),
        ],
    );
    let dropped = "---\n- !point x\n- y\n\n---\n";
    check_spans(
        dropped,
        &[
            ("Document", dropped),
            ("Frontmatter format=Yaml", "---\n- !point x\n- y\n\n---"),
            ("Mapping", "- !point x\n- y"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn a_plain_key_ends_before_the_comment_of_its_line() {
    check("---\nabc # c: d\n---\n", EMPTY_YAML);
    check(
        "---\na: b # c: d\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "b"
  MarkdownBlock
"#,
    );
    check(
        "---\na#b: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a#b
        Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn an_alias_reads_as_a_copy_of_the_scalar_its_anchor_names() {
    let source = "---\nauthor: &ada Ada\nreviewer: *ada\n---\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=author
        Scalar String "Ada"
      Pair key=reviewer
        Scalar String "Ada"
  MarkdownBlock
"#,
    );
    check_spans(
        source,
        &[
            ("Document", source),
            (
                "Frontmatter format=Yaml",
                "---\nauthor: &ada Ada\nreviewer: *ada\n---",
            ),
            ("Mapping", "author: &ada Ada\nreviewer: *ada"),
            ("Pair key=author", "author: &ada Ada"),
            ("Scalar String \"Ada\"", "Ada"),
            ("Pair key=reviewer", "reviewer: *ada"),
            ("Scalar String \"Ada\"", "*ada"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn an_alias_of_a_mapping_copies_every_value_in_it_each_spanning_the_alias() {
    let source = "---\nbase: &b\n  x: 1\n  y: [a, b]\ncopy: *b\n---\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=base
        Mapping
          Pair key=x
            Scalar Integer 1
          Pair key=y
            Sequence
              Scalar String "a"
              Scalar String "b"
      Pair key=copy
        Mapping
          Pair key=x
            Scalar Integer 1
          Pair key=y
            Sequence
              Scalar String "a"
              Scalar String "b"
  MarkdownBlock
"#,
    );
    let copy: Vec<(&str, &str)> = [
        "Mapping",
        "Pair key=x",
        "Scalar Integer 1",
        "Pair key=y",
        "Sequence",
        "Scalar String \"a\"",
        "Scalar String \"b\"",
    ]
    .into_iter()
    .map(|label| (label, "*b"))
    .collect();
    let mut expected = vec![
        ("Document", source),
        (
            "Frontmatter format=Yaml",
            "---\nbase: &b\n  x: 1\n  y: [a, b]\ncopy: *b\n---",
        ),
        ("Mapping", "base: &b\n  x: 1\n  y: [a, b]\ncopy: *b"),
        ("Pair key=base", "base: &b\n  x: 1\n  y: [a, b]"),
        ("Mapping", "x: 1\n  y: [a, b]"),
        ("Pair key=x", "x: 1"),
        ("Scalar Integer 1", "1"),
        ("Pair key=y", "y: [a, b]"),
        ("Sequence", "[a, b]"),
        ("Scalar String \"a\"", "a"),
        ("Scalar String \"b\"", "b"),
        ("Pair key=copy", "copy: *b"),
    ];
    expected.extend(copy);
    expected.push(("MarkdownBlock", ""));
    check_spans(source, &expected);
}

#[test]
fn an_anchor_and_its_alias_inside_flow_collections() {
    check(
        "---\ntags: [&t one, two, *t]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=tags
        Sequence
          Scalar String "one"
          Scalar String "two"
          Scalar String "one"
  MarkdownBlock
"#,
    );
    check(
        "---\na: {k: &v 1, l: *v}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=k
            Scalar Integer 1
          Pair key=l
            Scalar Integer 1
  MarkdownBlock
",
    );
}

/// The outline of a YAML frontmatter that holds `a` and `b`, each the
/// scalar labeled `scalar`, before an empty body.
fn a_and_b(scalar: &str) -> String {
    format!(
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar {scalar}
      Pair key=b
        Scalar {scalar}
  MarkdownBlock
"
    )
}

#[test]
fn a_tag_and_an_anchor_may_be_written_in_either_order() {
    for source in [
        "---\na: !!str &s 1\nb: *s\n---\n",
        "---\na: &s !!str 1\nb: *s\n---\n",
        "---\na: !!str\n  &s 1\nb: *s\n---\n",
        "---\na: &s\n  !!str 1\nb: *s\n---\n",
    ] {
        check(source, &a_and_b("String \"1\""));
    }
}

#[test]
fn an_anchor_on_an_empty_value_names_null() {
    check("---\na: &e\nb: *e\n---\n", &a_and_b("Null"));
    check(
        "---\nl:\n- &e\n- *e\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=l
        Sequence
          Scalar Null
          Scalar Null
  MarkdownBlock
",
    );
}

#[test]
fn an_anchor_on_a_key_names_the_key_and_its_alias_writes_that_key_again() {
    check(
        "---\n&k a: 1\n*k : 2\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a repeats=1
        Sequence
          Scalar Integer 1
          Scalar Integer 2
  MarkdownBlock
",
    );
    check(
        "---\nk:\n  &a x: 1\n  y: *a\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Mapping
          Pair key=x
            Scalar Integer 1
          Pair key=y
            Scalar String "x"
  MarkdownBlock
"#,
    );
}

#[test]
fn an_alias_as_a_key_is_the_text_of_the_scalar_its_anchor_names() {
    check(
        "---\nn: &n 0x1A\n? *n\n: v\n{*n : w}: x\nl: [*n : y]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=n
        Scalar Integer 26
      Pair key=0x1A
        Scalar String "v"
      Pair key=l
        Sequence
          Mapping
            Pair key=0x1A
              Scalar String "y"
  MarkdownBlock
"#,
    );
}

#[test]
fn an_alias_of_a_list_or_a_mapping_as_a_key_drops_its_pair() {
    for source in [
        "---\nm: &m {x: 1}\n*m : 2\nb: 2\n---\n",
        "---\nm: &m [1]\n? *m\n: v\nb: 2\n---\n",
    ] {
        let document = parse(source).document;
        let mapping = frontmatter_of(&document);
        assert_eq!(mapping.keys().collect::<Vec<_>>(), ["m", "b"], "{source:?}");
    }
    check(
        "---\nm: &m [1]\nf: {*m : 1, b: 2}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=m
        Sequence
          Scalar Integer 1
      Pair key=f
        Mapping
          Pair key=b
            Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn an_alias_copies_the_most_recent_anchor_of_its_name_before_it() {
    check(
        "---\na: &x 1\nb: &x 2\nc: *x\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=b
        Scalar Integer 2
      Pair key=c
        Scalar Integer 2
  MarkdownBlock
",
    );
    let inner = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Scalar Integer 1
          Scalar Integer 1
  MarkdownBlock
";
    check("---\na: &A [&A 1, *A]\n---\n", inner);
    let before = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 0
      Pair key=b
        Sequence
          Scalar Integer 0
          Scalar Integer 1
  MarkdownBlock
";
    check("---\na: &x 0\nb: [*x, &x 1]\n---\n", before);
    check("---\na: &x 0\nb:\n  [*x, &x 1]\n---\n", before);
    check("---\na: &x 0\nb:\n- [*x, &x 1]\n---\n", &before.replace(
        "        Sequence\n          Scalar Integer 0\n          Scalar Integer 1\n",
        "        Sequence\n          Sequence\n            Scalar Integer 0\n            Scalar Integer 1\n",
    ));
}

#[test]
fn an_alias_inside_the_value_its_anchor_names_drops_the_innermost_key_that_holds_it() {
    check("---\na: &A [*A]\nb: 2\n---\n", &only_b("Yaml"));
    check(
        "---\nx: &A 1\na: &A [*A]\nb: 2\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=x
        Scalar Integer 1
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
",
    );
    let emptied = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
";
    check("---\na: &A {x: [*A]}\nb: 2\n---\n", emptied);
    check("---\na: &A\n  x: *A\nb: 2\n---\n", emptied);
}

#[test]
fn an_alias_with_no_anchor_before_it_drops_the_key_that_holds_it() {
    check(
        "---\na: *B\nb: &B 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=b
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\na: 1\nb: *a\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
",
    );
    check("---\nx: [*y, &y \"]\"]\nb: 2\n---\n", &only_b("Yaml"));
    check(
        "---\na: {x: [*y, &y \"]\"], b: 2}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=b
            Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn anchors_on_values_list_entries_and_keys_are_read() {
    check(
        "---\nbase: &base 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=base
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\nitems:\n  - &first 1\n  - [*first]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Scalar Integer 1
          Sequence
            Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\n&key a: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_merge_key_is_a_key_like_any_other() {
    check(
        "---\nd: &d {a: 1}\ne:\n  <<: *d\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=d
        Mapping
          Pair key=a
            Scalar Integer 1
      Pair key=e
        Mapping
          Pair key=<<
            Mapping
              Pair key=a
                Scalar Integer 1
  MarkdownBlock
",
    );
}

/// A frontmatter of ten anchors, each a list of ten aliases of the one
/// before, the first a list of ten scalars, then `b: 2` and `c: *a0`.
fn ten_anchors_of_ten_aliases() -> String {
    let mut lines = vec!["a0: &a0 [x, x, x, x, x, x, x, x, x, x]".to_owned()];
    for level in 1_usize..10 {
        let aliases = vec![format!("*a{}", level - 1); 10].join(", ");
        lines.push(format!("a{level}: &a{level} [{aliases}]"));
    }
    format!("---\n{}\nb: 2\nc: *a0\n---\n", lines.join("\n"))
}

#[test]
fn aliases_that_copy_more_than_the_cap_drop_the_key_that_passes_it_and_every_later_alias() {
    on_two_megabytes(|| {
        let document = parse(&ten_anchors_of_ten_aliases()).document;
        let mapping = frontmatter_of(&document);
        // With the copy each anchor keeps, `a3` brings the copies to 24674
        // values, and the seventh alias in `a4` passes 100000.
        assert_eq!(
            mapping.keys().collect::<Vec<_>>(),
            ["a0", "a1", "a2", "a3", "b"]
        );
    });
}

/// A frontmatter whose `a` holds 190 anchored lists, each inside the one
/// before, around a list of `items` integers, then `b: 2`.
fn nested_anchors(items: usize) -> String {
    let opened: Vec<String> = (0_usize..190).map(|level| format!("&n{level} [")).collect();
    let list = vec!["1"; items].join(", ");
    let closed = "]".repeat(190);
    format!("---\na: {}{list}{closed}\nb: 2\n---\n", opened.concat())
}

#[test]
fn nested_anchors_that_keep_more_than_the_cap_drop_the_key_that_holds_them() {
    on_two_megabytes(|| {
        check(&nested_anchors(20_000), &only_b("Yaml"));
        // Around 100 integers, the anchors keep 37145 values.
        let document = parse(&nested_anchors(100)).document;
        let mapping = frontmatter_of(&document);
        assert_eq!(mapping.keys().collect::<Vec<_>>(), ["a", "b"]);
    });
}

/// A frontmatter whose `a` is an anchored string of 10000 bytes and whose
/// `b` is a list of `aliases` aliases of it.
fn aliases_of_a_long_string(aliases: usize) -> String {
    let text = "x".repeat(10_000);
    let list = vec!["*s"; aliases].join(", ");
    format!("---\na: &s {text}\nb: [{list}]\n---\n")
}

#[test]
fn aliases_that_copy_more_than_the_text_cap_drop_the_key_that_holds_them() {
    on_two_megabytes(|| {
        let only_a = format!(
            "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String \"{}\"
  MarkdownBlock
",
            "x".repeat(10_000)
        );
        // 10000 aliases copy 10000 values, under 100000: the bytes pass.
        check(&aliases_of_a_long_string(10_000), &only_a);
        // The anchor's own copy and 99 aliases come to 1000000 bytes, and a
        // hundredth alias passes.
        let document = parse(&aliases_of_a_long_string(99)).document;
        let mapping = frontmatter_of(&document);
        assert_eq!(mapping.keys().collect::<Vec<_>>(), ["a", "b"]);
        check(&aliases_of_a_long_string(100), &only_a);
    });
}

#[test]
fn anchors_and_aliases_under_the_copy_budget_read_as_before() {
    on_two_megabytes(|| {
        let copy = "        Mapping
          Pair key=draft
            Scalar Boolean false
          Pair key=lang
            Scalar String \"en\"
";
        check(
            "---\ndefaults: &d {draft: false, lang: en}\npost: *d\npage: *d\nnote: *d\n---\n",
            &format!(
                "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=defaults
{copy}      Pair key=post
{copy}      Pair key=page
{copy}      Pair key=note
{copy}  MarkdownBlock
"
            ),
        );
    });
}

/// The frontmatter of `document`, one of which an entry reads.
fn frontmatter_of(document: &RawDocument) -> &Mapping {
    let read = match document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => valid.frontmatter.as_ref(),
    };
    read.unwrap()
}

/// The keys of the frontmatter `source` reads, and the message and the text
/// of each entry it dropped. A strict parse fails with the error of the
/// first, which points into the entry, at its text or after.
fn kept_and_dropped(source: &str) -> (Vec<String>, Vec<(String, &str)>) {
    let parsed = parse(source);
    let keys = frontmatter_of(&parsed.document).keys().cloned().collect();
    let dropped: Vec<(String, &str)> = parsed
        .dropped
        .iter()
        .map(|drop| (drop.message.clone(), &source[drop.span.clone()]))
        .collect();
    let strict = parse_with::<Plain>(source, &ParseOptions::strict());
    match parsed.dropped.first() {
        Some(first) => {
            let error = strict.unwrap_err();
            assert_eq!(error.message, first.message, "{source:?}");
            assert!(first.span.start <= error.span.start, "{source:?}");
        }
        None => assert!(strict.is_ok(), "{source:?}"),
    }
    (keys, dropped)
}

/// `keys` kept and one entry dropped, with `message`, at `text`.
fn one_dropped<'src>(
    keys: &[&str],
    message: &str,
    text: &'src str,
) -> (Vec<String>, Vec<(String, &'src str)>) {
    let kept = keys.iter().map(|&key| key.to_owned()).collect();
    (kept, vec![(message.to_owned(), text)])
}

#[test]
fn a_yaml_entry_that_does_not_read_is_dropped_up_to_the_next_line_at_its_column() {
    let source = "---\ntitle: Notes\ntags: [a, b\ndate: 2026-10-17\n---\n# Body\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "Notes"
      Pair key=date
        Scalar Date 2026-10-17
  MarkdownBlock
    Section
      Heading
        Text "Body"
"#,
    );
    let flow = "a line of a flow collection must be indented more than its key";
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["title", "date"], flow, "tags: [a, b")
    );
    let quoted = "a line inside a quoted string must be indented more than its key or `-`";
    assert_eq!(
        kept_and_dropped("---\ntitle: \"Notes\ndate: 2026-10-17\n---\n# Body\n"),
        one_dropped(&["date"], quoted, "title: \"Notes")
    );
    assert_eq!(
        kept_and_dropped("---\na: 1\nabc # c: d\nb: 2\n---\n# Body\n"),
        one_dropped(&["a", "b"], "expected `key: value`", "abc # c: d")
    );
}

#[test]
fn a_nested_yaml_entry_that_does_not_read_drops_only_itself() {
    let source = "---\nowner:\n  name: Ada\n  @handle: ada\n  role: dev\n---\n# Body\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=role
            Scalar String "dev"
  MarkdownBlock
    Section
      Heading
        Text "Body"
"#,
    );
    let at = "a plain key cannot start with `@`; quote it";
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["owner"], at, "@handle: ada")
    );
}

#[test]
fn a_yaml_list_entry_that_does_not_read_drops_only_itself() {
    let flow = "a line of a flow collection must be indented more than its key";
    let source = "---\nitems:\n- [a\n- b\n- c\n---\n";
    check(
        source,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Scalar String "b"
          Scalar String "c"
  MarkdownBlock
"#,
    );
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["items"], flow, "- [a")
    );
    let tab = "a tab cannot indent a line; use spaces";
    assert_eq!(
        kept_and_dropped("---\nitems:\n- a\n\tb: 1\nc: 2\n---\n"),
        one_dropped(&["items", "c"], tab, "\tb: 1")
    );
}

#[test]
fn a_pair_of_a_flow_mapping_over_several_lines_is_dropped_up_to_the_next_line_at_its_column() {
    let source = "---\n{\n  title: Notes,\n  tags: [a, b,\n  date: 2026-10-17\n}\n---\n";
    let list = "expected `,` or `]` in the list";
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["title", "date"], list, "tags: [a, b,")
    );
    let nested = "---\na: {x: [1,\n    y: 2}\nb: 3\n---\n";
    let mapping = frontmatter_of(&parse(nested).document).clone();
    assert_eq!(mapping.keys().collect::<Vec<_>>(), ["a", "b"]);
    let inner = if let Value::Mapping(inner) = &mapping["a"] {
        inner.keys().collect()
    } else {
        Vec::new()
    };
    assert_eq!(inner, ["y"]);
}

#[test]
fn text_where_a_comma_is_expected_in_a_flow_mapping_over_several_lines_is_dropped() {
    let separator = "expected `,` or `}` in the mapping";
    let missing = "---\n{\n  title: Notes\n  date: 2026-10-17\n}\n---\n";
    assert_eq!(
        kept_and_dropped(missing),
        one_dropped(&["title"], separator, "date: 2026-10-17")
    );
    let quoted = "---\n{\n  a: \"x\n  y\" junk,\n  b: 1\n}\n---\n";
    check(
        quoted,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "x y"
      Pair key=b
        Scalar Integer 1
  MarkdownBlock
"#,
    );
    assert_eq!(
        kept_and_dropped(quoted),
        one_dropped(&["a", "b"], separator, "junk,")
    );
    for (source, text) in [(missing, "date"), (quoted, "junk")] {
        let at = source.find(text).unwrap();
        let strict = parse_with::<Plain>(source, &ParseOptions::strict());
        assert_eq!(strict.unwrap_err(), ParseError::new(separator, at..at));
    }
    assert_eq!(
        kept_and_dropped("---\n{\n  a: \"x\" junk,\n  b: 1\n}\n---\n"),
        one_dropped(&["b"], separator, "a: \"x\" junk,")
    );
    let repeated = "---\n{\n  a: 1,\n  a: \"x\" junk,\n  b: 2\n}\n---\n";
    check(
        repeated,
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
",
    );
    assert_eq!(
        kept_and_dropped(repeated),
        one_dropped(&["a", "b"], separator, "a: \"x\" junk,")
    );
}

#[test]
fn a_missing_comma_in_a_flow_mapping_on_one_line_fails_the_mapping() {
    let separator = "expected `,` or `}` in the mapping";
    let source = "---\n{a: \"x\" b: 1}\n---\n";
    let at = source.find('b').unwrap();
    assert_eq!(broken(source).error, ParseError::new(separator, at..at));
    assert_eq!(
        kept_and_dropped("---\nm: {a: \"x\" b}\nc: 1\n---\n"),
        one_dropped(&["c"], separator, "m: {a: \"x\" b}")
    );
}

#[test]
fn a_broken_pair_of_a_flow_mapping_that_closes_on_its_line_fails_the_mapping() {
    let at_sign = "a plain value cannot start with `@`; quote it";
    let separator = "expected `,` or `}` in the mapping";
    for (source, message, text, at) in [
        (
            "---\nm: {a: @x}\n  c: 1\nd: 2\n---\n",
            at_sign,
            "m: {a: @x}\n  c: 1",
            11..12,
        ),
        (
            "---\nm: {a: @x, b: 1}\n  c: 1\nd: 2\n---\n",
            at_sign,
            "m: {a: @x, b: 1}\n  c: 1",
            11..12,
        ),
        (
            "---\nm: {a: \"x\" b}\n  c: 1\nd: 2\n---\n",
            separator,
            "m: {a: \"x\" b}\n  c: 1",
            15..15,
        ),
    ] {
        check(
            source,
            "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=d
        Scalar Integer 2
  MarkdownBlock
",
        );
        assert_eq!(kept_and_dropped(source), one_dropped(&["d"], message, text));
        let strict = parse_with::<Plain>(source, &ParseOptions::strict());
        assert_eq!(strict.unwrap_err(), ParseError::new(message, at));
    }
}

#[test]
fn a_yaml_line_indented_more_than_the_value_above_it_drops_that_entry() {
    let deeper = "this line is indented more than the keys above it";
    assert_eq!(
        kept_and_dropped("---\na: 1\n  b: 2\nc: 3\n---\n"),
        one_dropped(&["c"], deeper, "a: 1\n  b: 2")
    );
    let shallower = "this line is indented less than the first key";
    assert_eq!(
        kept_and_dropped("---\n  a: 1\nb: 2\n---\n"),
        one_dropped(&["a"], shallower, "b: 2")
    );
    let after = "expected the end of the frontmatter after the mapping";
    assert_eq!(
        kept_and_dropped("---\n{a: 1}\nb: 2\n---\n"),
        one_dropped(&["a"], after, "b: 2")
    );
}

#[test]
fn a_value_or_a_list_entry_before_the_keys_of_a_yaml_frontmatter_is_dropped() {
    let no_key = "expected `key: value`";
    let list_entry = "expected a key, found a list entry";
    assert_eq!(
        kept_and_dropped("---\nhello\nc: 1\n---\n"),
        one_dropped(&["c"], no_key, "hello")
    );
    assert_eq!(
        kept_and_dropped("---\n- a\nc: 1\n---\n"),
        one_dropped(&["c"], list_entry, "- a")
    );
    assert_eq!(
        kept_and_dropped("---\n- !point a\nc: 1\n---\n"),
        one_dropped(&["c"], list_entry, "- !point a")
    );
    assert_eq!(
        kept_and_dropped("---\n!!int abc\n\nb: 1\n\n---\n"),
        one_dropped(&["b"], no_key, "!!int abc")
    );
    let tab = "a tab cannot indent a line; use spaces";
    assert_eq!(
        kept_and_dropped("---\n\ta: 1\nb: 2\n---\n"),
        one_dropped(&["b"], tab, "\ta: 1")
    );
}

#[test]
fn a_toml_statement_that_does_not_read_is_dropped_up_to_the_next_statement() {
    let source = "+++\ntitle = Notes\ndate = 2026-10-17\n+++\n# Body\n";
    check(
        source,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=date
        Scalar Date 2026-10-17
  MarkdownBlock
    Section
      Heading
        Text "Body"
"#,
    );
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["date"], "`Notes` is not a value", "title = Notes")
    );
    let array = "expected `,` or `]` in the array";
    assert_eq!(
        kept_and_dropped("+++\ntags = [\"a\", \"b\"\ndate = 2026-10-17\n+++\n# Body\n"),
        one_dropped(&["date"], array, "tags = [\"a\", \"b\"")
    );
    let end = "expected the end of the line after the value";
    assert_eq!(
        kept_and_dropped("+++\na = 1 2\nb = 2\n+++\n"),
        one_dropped(&["b"], end, "a = 1 2")
    );
}

#[test]
fn a_toml_header_that_does_not_read_is_dropped_and_a_string_left_open_drops_the_rest() {
    let header = "expected `]` to close the table header";
    assert_eq!(
        kept_and_dropped("+++\nname = \"Ada\"\n[owner\n[t]\nx = 1\n+++\n"),
        one_dropped(&["name", "t"], header, "[owner")
    );
    let source = "+++\n[a]\nx = 1\n[owner\ny = 2\n+++\n";
    check(
        source,
        "Document
  Frontmatter format=Toml
    Mapping
      Pair key=a
        Mapping
          Pair key=x
            Scalar Integer 1
  MarkdownBlock
",
    );
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["a"], header, "[owner\ny = 2")
    );
    let open = "the string is not closed";
    assert_eq!(
        kept_and_dropped("+++\na = 1\nt = \"\"\"never\nb = 2\n+++\n"),
        one_dropped(&["a"], open, "t = \"\"\"never\nb = 2")
    );
}

#[test]
fn a_toml_pair_that_does_not_read_is_dropped_up_to_where_its_value_ends() {
    let source = "+++\na = [\n[1],\n@\n]\nb = 2\n+++\n";
    check(
        source,
        "Document
  Frontmatter format=Toml
    Mapping
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
",
    );
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["b"], "`@` is not a value", "a = [\n[1],\n@\n]")
    );
    assert_eq!(
        kept_and_dropped("+++\na = {x = 1,\ny = 2}\nb = 3\n+++\n"),
        one_dropped(&["b"], "expected a key", "a = {x = 1,\ny = 2}")
    );
    assert_eq!(
        kept_and_dropped("+++\na = [\n[1],\nb = 2\n+++\n"),
        one_dropped(&["b"], "`b` is not a value", "a = [\n[1],")
    );
    assert_eq!(
        kept_and_dropped("+++\npaths = [\"C:\\dev\"]\ntitle = \"x\"\n+++\n"),
        one_dropped(
            &["title"],
            "unknown escape in the string",
            "paths = [\"C:\\dev\"]"
        )
    );
}

#[test]
fn a_dropped_toml_table_ends_at_the_next_header_after_a_bracket_left_open() {
    let tree = "Document
  Frontmatter format=Toml
    Mapping
      Pair key=k
        Scalar Integer 0
      Pair key=u
        Mapping
          Pair key=z
            Scalar Integer 3
  MarkdownBlock
";
    let source = "+++\nk = 0\n[t\nx = [\ny = 1\n[u]\nz = 3\n+++\n";
    check(source, tree);
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(
            &["k", "u"],
            "expected `]` to close the table header",
            "[t\nx = [\ny = 1"
        )
    );
    let header = format!("[{}t]", "a.".repeat(VALUE_CAP));
    let past_cap = format!("+++\nk = 0\n{header}\nx = [\ny = 1\n[u]\nz = 3\n+++\n");
    check(&past_cap, tree);
    assert_eq!(
        kept_and_dropped(&past_cap),
        one_dropped(
            &["k", "u"],
            &format!("the value nests deeper than {VALUE_CAP} levels"),
            &format!("{header}\nx = [\ny = 1")
        )
    );
}

#[test]
fn a_toml_pair_past_the_value_cap_whose_bracket_never_closes_ends_before_the_next_pair() {
    on_two_megabytes(|| {
        let past = VALUE_CAP + 1;
        let cap = format!("the value nests deeper than {VALUE_CAP} levels");
        let tree = "Document
  Frontmatter format=Toml
    Mapping
      Pair key=k
        Scalar Integer 0
      Pair key=b
        Scalar Integer 2
      Pair key=u
        Mapping
          Pair key=z
            Scalar Integer 3
  MarkdownBlock
";
        let open = format!("deep = {}", "[".repeat(past));
        let closed = format!("{open}1{}", "]".repeat(past));
        for deep in [&open, &closed] {
            let source = format!("+++\nk = 0\n{deep}\nb = 2\n[u]\nz = 3\n+++\n");
            check(&source, tree);
            assert_eq!(
                kept_and_dropped(&source),
                one_dropped(&["k", "b", "u"], &cap, deep)
            );
        }
        let under_s = format!("+++\nk = 0\n[s]\n{open}\nb = 2\n[u]\nz = 3\n+++\n");
        check(
            &under_s,
            "Document
  Frontmatter format=Toml
    Mapping
      Pair key=k
        Scalar Integer 0
      Pair key=s
        Mapping
          Pair key=b
            Scalar Integer 2
      Pair key=u
        Mapping
          Pair key=z
            Scalar Integer 3
  MarkdownBlock
",
        );
        assert_eq!(
            kept_and_dropped(&under_s),
            one_dropped(&["k", "s", "u"], &cap, &open)
        );
    });
}

#[test]
fn a_toml_statement_that_does_not_read_passes_a_string_whole_past_a_bad_escape() {
    let source = "+++\na = 1 2\n\"\\q\"\"\"\nb = 2\n+++\n";
    check(
        source,
        "Document
  Frontmatter format=Toml
    Mapping
      Pair key=b
        Scalar Integer 2
  MarkdownBlock
",
    );
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(
            &["b"],
            "expected the end of the line after the value",
            "a = 1 2\n\"\\q\"\"\""
        )
    );
}

#[test]
fn the_drops_of_a_key_a_tag_a_circular_or_undefined_alias_are_listed() {
    let key = "a key cannot be a list or a mapping";
    assert_eq!(
        kept_and_dropped("---\n[x]: 1\nb: 2\n---\n"),
        one_dropped(&["b"], key, "[x]: 1")
    );
    assert_eq!(
        kept_and_dropped("---\na: {[x]: 1, b: 2}\n---\n"),
        one_dropped(&["a"], key, "[x]: 1")
    );
    let tag = "the tag `!point` is not one arky reads";
    assert_eq!(
        kept_and_dropped("---\na: !point 1\nb: 2\n---\n"),
        one_dropped(&["b"], tag, "a: !point 1")
    );
    let circular = "the alias `*A` lies inside the value its anchor names";
    assert_eq!(
        kept_and_dropped("---\na: &A [*A]\nb: 2\n---\n"),
        one_dropped(&["b"], circular, "a: &A [*A]")
    );
    let undefined = "no anchor `&B` is defined before this alias";
    assert_eq!(
        kept_and_dropped("---\na: *B\nb: 2\n---\n"),
        one_dropped(&["b"], undefined, "a: *B")
    );
}

#[test]
fn the_drop_of_a_value_past_the_value_cap_is_listed() {
    on_two_megabytes(|| {
        let past = VALUE_CAP + 1;
        let deep = format!("{}1{}", "[".repeat(past), "]".repeat(past));
        let cap = format!("the value nests deeper than {VALUE_CAP} levels");
        let yaml = format!("---\ndeep: {deep}\nb: 2\n---\n");
        assert_eq!(
            kept_and_dropped(&yaml),
            one_dropped(&["b"], &cap, &format!("deep: {deep}"))
        );
        let toml = format!("+++\ndeep = {deep}\nb = 2\n+++\n");
        assert_eq!(
            kept_and_dropped(&toml),
            one_dropped(&["b"], &cap, &format!("deep = {deep}"))
        );
    });
}

#[test]
fn an_entry_dropped_inside_a_dropped_entry_or_a_lookahead_is_not_listed_again() {
    let misfit = "a mapping is not a value of the tag `!!seq`";
    assert_eq!(
        kept_and_dropped("---\na: !!seq\n  b: !point 1\n  c: 2\nd: 1\n---\n"),
        one_dropped(&["d"], misfit, "a: !!seq\n  b: !point 1\n  c: 2")
    );
    let key = "a key cannot be a list or a mapping";
    assert_eq!(
        kept_and_dropped("---\n{x: !point 1}: 3\nb: 2\n---\n"),
        one_dropped(&["b"], key, "{x: !point 1}: 3")
    );
}

#[test]
fn an_anchor_named_inside_a_dropped_yaml_entry_is_forgotten_with_it() {
    let at = "a plain value cannot start with `@`; quote it";
    let source = "---\na: [&x 1, @bad]\nb: *x\n---\n";
    assert_eq!(broken(source).error.message, at);
    let (_, dropped) = yaml::read(source, 4..source.len() - 4).unwrap();
    let alias = source.find("*x").unwrap();
    assert_eq!(dropped[1].error.span, alias..alias + 2);
    let with_c = "---\na: [&x 1, @bad]\nb: *x\nc: 3\n---\n";
    check(
        with_c,
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=c
        Scalar Integer 3
  MarkdownBlock
",
    );
    let undefined = "no anchor `&x` is defined before this alias";
    assert_eq!(
        kept_and_dropped(with_c),
        (
            vec!["c".to_owned()],
            vec![
                (at.to_owned(), "a: [&x 1, @bad]"),
                (undefined.to_owned(), "b: *x")
            ]
        )
    );
}

#[test]
fn an_anchor_redefined_inside_a_dropped_yaml_entry_names_its_earlier_value_again() {
    let source = "---\na: &x 1\nb: [&x 2, @bad]\nc: *x\n---\n";
    check(
        source,
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=c
        Scalar Integer 1
  MarkdownBlock
",
    );
    let at = "a plain value cannot start with `@`; quote it";
    assert_eq!(
        kept_and_dropped(source),
        one_dropped(&["a", "c"], at, "b: [&x 2, @bad]")
    );
}

#[test]
fn a_dropped_list_entry_flow_pair_or_pair_with_a_collection_key_forgets_its_anchors() {
    check(
        "---\na:\n- &x 1\n- [&x 2, @bad]\nb: *x\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Scalar Integer 1
      Pair key=b
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\na: {p: &x 1, q: [&x 2, *y], r: *x}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=p
            Scalar Integer 1
          Pair key=r
            Scalar Integer 1
  MarkdownBlock
",
    );
    let key = "a key cannot be a list or a mapping";
    let undefined = "no anchor `&x` is defined before this alias";
    assert_eq!(
        kept_and_dropped("---\n[&x 1]: 2\nb: *x\nc: 3\n---\n"),
        (
            vec!["c".to_owned()],
            vec![
                (key.to_owned(), "[&x 1]: 2"),
                (undefined.to_owned(), "b: *x")
            ]
        )
    );
}

#[test]
fn every_drop_is_listed_in_file_order() {
    let (keys, dropped) = kept_and_dropped("---\na: !point 1\nb: 2\nc: [x\nd: 4\n[y]: 5\n---\n");
    assert_eq!(keys, ["b", "d"]);
    let texts: Vec<&str> = dropped.iter().map(|(_, text)| *text).collect();
    assert_eq!(texts, ["a: !point 1", "c: [x", "[y]: 5"]);
}

#[test]
fn a_frontmatter_of_which_no_entry_reads_is_broken_with_the_error_of_the_first() {
    let refused = |source: &str| broken(source).error.message;
    let open = "---\ntitle: \"Notes\n---\n# Body\n";
    let error = broken(open).error;
    assert_eq!(error.message, "the string is not closed");
    assert_eq!(&open[error.span.start..], "---\n# Body\n");
    let at = "a plain key cannot start with `@`; quote it";
    assert_eq!(refused("---\n{\n  @a: 1\n}\n---\n"), at);
    assert_eq!(refused("---\na: !!seq\n  @x: 1\n---\n"), at);
    assert_eq!(
        refused("---\n- !!int abc\n---\n"),
        "the frontmatter must be a mapping"
    );
    assert_eq!(
        refused("+++\ntitle = Notes\n+++\n"),
        "`Notes` is not a value"
    );
}

#[test]
fn a_flow_mapping_that_does_not_read_is_kept_as_its_text_with_its_error() {
    let source = "---\n{a: 1, b: [}\n---\n# A\n";
    check(
        source,
        r#"InvalidDocument
  InvalidFrontmatter format=Yaml error="expected `,` or `]` in the list"
  MarkdownBlock
    Section
      Heading
        Text "A"
"#,
    );
    let at = source.find('}').unwrap();
    let error = ParseError::new("expected `,` or `]` in the list", at..at);
    assert_eq!(
        broken(source),
        BrokenFrontmatter {
            error: error.clone(),
            format: FrontmatterFormat::Yaml,
            span: 0..source.find("\n# A").unwrap(),
            text: "{a: 1, b: [}".to_owned(),
        }
    );
    let parsed = parse(source);
    assert_eq!(parsed.dropped, slice::from_ref(&error));
    let invalid = match parsed.document {
        RawDocument::Invalid(InvalidDocument {
            body: _,
            frontmatter,
        }) => Some(frontmatter),
        RawDocument::Valid(_) => None,
    };
    assert_eq!(
        invalid,
        Some(InvalidFrontmatter {
            error: error.message,
            format: FrontmatterFormat::Yaml,
            text: "{a: 1, b: [}".to_owned(),
        })
    );
    assert_eq!(
        broken("---\r\n{a: 1, b: [}\r\n---\r\n").text,
        "{a: 1, b: [}"
    );
}

#[test]
fn a_yaml_frontmatter_is_broken_only_when_every_entry_is_dropped() {
    let flow = "a line of a flow collection must be indented more than its key";
    let unclosed = broken("---\na: [\nb: {\n---\n");
    assert_eq!(
        (unclosed.error.message.as_str(), unclosed.text.as_str()),
        (flow, "a: [\nb: {")
    );
    let tag = "the tag `!point` is not one arky reads";
    let tagged = "---\na: !point 1\nb: !point 2\n---\n";
    assert_eq!(broken(tagged).error.message, tag);
    let at = tagged.find('!').unwrap();
    assert_eq!(broken(tagged).error.span, at..at + "!point".len());
    let (keys, dropped) = kept_and_dropped("---\na: !point 1\nb: 2\nc: [x\n---\n");
    assert_eq!(keys, ["b"]);
    let texts: Vec<&str> = dropped.iter().map(|(_, text)| *text).collect();
    assert_eq!(texts, ["a: !point 1", "c: [x"]);
}

#[test]
fn a_toml_frontmatter_whose_first_entry_opens_a_string_it_never_closes_is_broken() {
    let source = "+++\na = \"\"\"never closed\n+++\n# A\n";
    check(
        source,
        r#"InvalidDocument
  InvalidFrontmatter format=Toml error="the string is not closed"
  MarkdownBlock
    Section
      Heading
        Text "A"
"#,
    );
    assert_eq!(broken(source).text, "a = \"\"\"never closed");
}

#[test]
fn an_empty_frontmatter_or_one_that_holds_no_key_is_an_empty_mapping() {
    for source in [
        "---\n---\n",
        "+++\n+++\n",
        "---\nabc\n---\n",
        "---\n- a\n- b\n---\n",
        "---\n[a, !point b]\n---\n",
    ] {
        let document = parse(source).document;
        assert!(frontmatter_of(&document).is_empty(), "{source:?}");
    }
}

#[test]
fn a_strict_parse_fails_with_the_error_of_a_broken_frontmatter() {
    for source in [
        "---\n{a: 1, b: [}\n---\n# A\n",
        "---\na: !point 1\nb: !point 2\n---\n",
        "+++\na = \"\"\"never closed\n+++\n# A\n",
    ] {
        let strict = parse_with::<Plain>(source, &ParseOptions::strict());
        assert_eq!(strict.unwrap_err(), broken(source).error, "{source:?}");
    }
}

#[test]
fn a_frontmatter_with_no_drop_lists_none() {
    for source in [
        "---\ntitle: Notes\ntags: [a, b]\n---\n",
        "+++\ntitle = \"Notes\"\n+++\n",
        "---\n---\n",
        "---\n- a\n---\n",
    ] {
        let parsed = parse(source);
        assert!(
            matches!(parsed.document, RawDocument::Valid(_)),
            "{source:?}"
        );
        assert!(parsed.dropped.is_empty(), "{source:?}");
    }
}

#[test]
fn an_entry_that_does_not_read_is_listed_beside_the_valid_document_with_its_span() {
    let source = "---\nstatus: open\nbad: [\n---\n# Task\n";
    let parsed = parse(source);
    let read = frontmatter_of(&parsed.document);
    assert_eq!(read.keys().collect::<Vec<_>>(), ["status"]);
    assert_eq!(read["status"], Value::String("open".to_owned()));
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].span, 17..23);
    assert_eq!(&source[17..23], "bad: [");
    let strict = parse_with::<Plain>(source, &ParseOptions::strict()).unwrap_err();
    assert_eq!(strict.message, parsed.dropped[0].message);
}

#[test]
fn a_frontmatter_no_entry_of_reads_makes_an_invalid_document_written_back_as_read() {
    let source = "---\n: : :\n---\ntext\n";
    let parsed = parse(source);
    let invalid = match &parsed.document {
        RawDocument::Invalid(invalid) => Some(&invalid.frontmatter),
        RawDocument::Valid(_) => None,
    };
    assert_eq!(
        invalid,
        Some(&InvalidFrontmatter {
            error: "a plain value cannot start with `:` and a space; quote it".to_owned(),
            format: FrontmatterFormat::Yaml,
            text: ": : :".to_owned(),
        })
    );
    assert_eq!(parsed.dropped, slice::from_ref(&broken(source).error));
    for options in [
        WriteOptions::default(),
        WriteOptions {
            frontmatter: FrontmatterFormat::Toml,
        },
    ] {
        assert_eq!(write(&parsed.document, &options).unwrap(), source);
    }
}
