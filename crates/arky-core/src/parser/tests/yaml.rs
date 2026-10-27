use core::ops::Range;

use chrono::{DateTime, NaiveDate, NaiveTime};

use super::{check, check_spans, located_frontmatter};
use crate::parser::ast::{Mapping, Value};
use crate::parser::error::ParseError;
use crate::parser::{read_toml, read_toml_lines, read_yaml, read_yaml_lines, toml, yaml};

/// `read_toml` or `read_yaml`.
type Reader = fn(&str, Range<usize>) -> Result<Mapping, ParseError>;

#[test]
fn a_note_frontmatter_with_every_scalar_type_and_both_list_styles() {
    check(
        r#"---
type: agent-lesson
title: 'It''s "quoted"'
description: ''
date: 2026-09-26
created: 2026-09-26T17:08:20.162234780Z
priority: 2
weight: 0.5
done: false
remargin_last_activity: null
tags:
- design
- requirements
aliases: [Parser, "The parser"]
---
# Title
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=type
        Scalar String "agent-lesson"
      Pair key=title
        Scalar String "It's \"quoted\""
      Pair key=description
        Scalar String ""
      Pair key=date
        Scalar Date 2026-09-26
      Pair key=created
        Scalar OffsetDateTime 2026-09-26T17:08:20.162234780Z
      Pair key=priority
        Scalar Integer 2
      Pair key=weight
        Scalar Float 0.5
      Pair key=done
        Scalar Boolean false
      Pair key=remargin_last_activity
        Scalar Null
      Pair key=tags
        Sequence
          Scalar String "design"
          Scalar String "requirements"
      Pair key=aliases
        Sequence
          Scalar String "Parser"
          Scalar String "The parser"
  MarkdownBlock
    Section
      Heading
        Text "Title"
"#,
    );
}

#[test]
fn nested_mappings_indented_lists_comments_and_keys_with_no_value() {
    check(
        r#"---
# who owns it
owner:
  name: Ada
  contact:
    email: ada@example.com
"quoted key": yes   # a string: only true and false are booleans
steps:
  - first
  -
  - third
nothing:
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=contact
            Mapping
              Pair key=email
                Scalar String "ada@example.com"
      Pair key=quoted key
        Scalar String "yes"
      Pair key=steps
        Sequence
          Scalar String "first"
          Scalar Null
          Scalar String "third"
      Pair key=nothing
        Scalar Null
  MarkdownBlock
"#,
    );
}

#[test]
fn a_double_quoted_string_reads_its_escapes() {
    check(
        r#"---
text: "tab\there \"quoted\" A"
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=text
        Scalar String "tab\there \"quoted\" A"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_plain_scalar_is_typed_by_how_it_is_written() {
    check(
        "\
---
negative: -17
hex: 0x1F
exponent: 1e3
version: 1.2.3
time: 07:32:00
tilde: ~
truthy: True
leading_dot: .5
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=negative
        Scalar Integer -17
      Pair key=hex
        Scalar Integer 31
      Pair key=exponent
        Scalar Float 1000
      Pair key=version
        Scalar String "1.2.3"
      Pair key=time
        Scalar String "07:32:00"
      Pair key=tilde
        Scalar Null
      Pair key=truthy
        Scalar Boolean true
      Pair key=leading_dot
        Scalar Float 0.5
  MarkdownBlock
"#,
    );
}

#[test]
fn a_float_may_have_no_digit_before_its_dot_or_none_after_it() {
    check(
        "\
---
a: .5
b: 5.
c: -.5e3
d: 5.e3
e: .
f: 1e3
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Float 0.5
      Pair key=b
        Scalar Float 5
      Pair key=c
        Scalar Float -500
      Pair key=d
        Scalar Float 5000
      Pair key=e
        Scalar String "."
      Pair key=f
        Scalar Float 1000
  MarkdownBlock
"#,
    );
}

#[test]
fn digits_too_large_for_64_bits_read_under_a_float_or_a_text_tag_or_quoted() {
    check(
        "\
---
largest: 9223372036854775807
float: !!float 99999999999999999999
quoted: \"99999999999999999999\"
text: !!str 99999999999999999999
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=largest
        Scalar Integer 9223372036854775807
      Pair key=float
        Scalar Float 100000000000000000000
      Pair key=quoted
        Scalar String "99999999999999999999"
      Pair key=text
        Scalar String "99999999999999999999"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_key_of_digits_too_large_for_64_bits_is_its_text_whether_implicit_or_explicit() {
    let mapping = r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=99999999999999999999
        Scalar String "x"
  MarkdownBlock
"#;
    check("---\n99999999999999999999: x\n---\n", mapping);
    check("---\n? 99999999999999999999\n: x\n---\n", mapping);
}

#[test]
fn a_list_entry_may_hold_a_mapping_and_mappings_nest_lists_of_mappings() {
    check(
        "\
---
people:
  - name: Ada
    role: host
  - name: Grace
    tags: [review]
reactions:
  thumbs:
    - author: ada
      ts: 2026-10-01T10:00:00Z
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=people
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
            Pair key=role
              Scalar String "host"
          Mapping
            Pair key=name
              Scalar String "Grace"
            Pair key=tags
              Sequence
                Scalar String "review"
      Pair key=reactions
        Mapping
          Pair key=thumbs
            Sequence
              Mapping
                Pair key=author
                  Scalar String "ada"
                Pair key=ts
                  Scalar OffsetDateTime 2026-10-01T10:00:00Z
  MarkdownBlock
"#,
    );
}

#[test]
fn the_readers_take_a_range_of_a_larger_text_and_keep_its_offsets() {
    let source = "intro\nname: Ada\nrole: host\nmore";
    let mapping = read_yaml(source, 6..27).unwrap();
    let keys: Vec<&str> = mapping.keys().map(String::as_str).collect();
    assert_eq!(keys, ["name", "role"]);
    let (located_yaml, _) = yaml::read(source, 6..27).unwrap();
    assert_eq!(&source[located_yaml.span.clone()], "name: Ada\nrole: host");
    assert_eq!(located_yaml.into_model(), mapping);

    let text = "x\nname = \"Ada\"\ny";
    let table = read_toml(text, 2..15).unwrap();
    let (located_toml, _) = toml::read(text, 2..15).unwrap();
    assert_eq!(
        &text[located_toml.pairs["name"].span.clone()],
        "name = \"Ada\""
    );
    assert_eq!(located_toml.into_model(), table);
}

#[test]
fn the_readers_give_plain_values_with_a_key_written_twice_as_one_list() {
    let source = "status: open\npriority: 2\nbig: 1e400\nx: .inf\ntag: a\ntag: b\n";
    let mapping = read_yaml(source, 0..source.len()).unwrap();
    assert_eq!(
        mapping,
        Mapping::from([
            ("status".to_owned(), Value::String("open".to_owned())),
            ("priority".to_owned(), Value::Integer(2)),
            ("big".to_owned(), Value::String("1e400".to_owned())),
            ("x".to_owned(), Value::String(".inf".to_owned())),
            (
                "tag".to_owned(),
                Value::Sequence(vec![
                    Value::String("a".to_owned()),
                    Value::String("b".to_owned()),
                ])
            ),
        ])
    );

    let text =
        "a = 2026-10-20\nb = 2026-10-20T09:00:00\nc = 2026-10-20T09:00:00+02:00\nd = 07:32:00";
    let date = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let nine = date.and_hms_opt(9, 0, 0).unwrap();
    let offset = DateTime::parse_from_rfc3339("2026-10-20T09:00:00+02:00").unwrap();
    assert_eq!(
        read_toml(text, 0..text.len()).unwrap(),
        Mapping::from([
            ("a".to_owned(), Value::Date(date)),
            ("b".to_owned(), Value::DateTime(nine)),
            ("c".to_owned(), Value::OffsetDateTime(offset)),
            (
                "d".to_owned(),
                Value::Time(NaiveTime::from_hms_opt(7, 32, 0).unwrap())
            ),
        ])
    );
}

#[test]
fn the_readers_fail_with_their_first_error_and_drop_nothing() {
    let refused = |source: &str, read: Reader| {
        let error = read(source, 0..source.len()).unwrap_err();
        (error.message, source[error.span].to_owned())
    };
    let flow = "a line of a flow collection must be indented more than its key";
    let yaml = "title: Notes\ntags: [a, b\ndate: 2026-10-17";
    let error = read_yaml(yaml, 0..yaml.len()).unwrap_err();
    assert_eq!(error.message, flow);
    let date = yaml.find("date").unwrap();
    assert_eq!(error.span, date..date);
    let tag = "the tag `!point` is not one arky reads";
    assert_eq!(
        refused("a: !point 1\nb: 2", read_yaml),
        (tag.to_owned(), "!point".to_owned())
    );
    assert_eq!(
        refused("title = Notes\ndate = 2026-10-17", read_toml),
        ("`Notes` is not a value".to_owned(), "Notes".to_owned())
    );
    let source = "> title = Notes\n> date = 2026-10-17\n> a: !point 1\n> b: 2\n";
    let line = |text: &str| {
        let start = source.find(text).unwrap();
        start..start + text.len()
    };
    let toml = read_toml_lines(source, &[line("title = Notes"), line("date = 2026-10-17")]);
    assert_eq!(toml.unwrap_err().span, line("Notes"));
    let yaml_lines = read_yaml_lines(source, &[line("a: !point 1"), line("b: 2")]);
    assert_eq!(yaml_lines.unwrap_err().span, line("!point"));
}

#[test]
fn a_key_written_twice_holds_a_list_of_both_values() {
    check(
        "---\ntitle: Draft\ntitle: Final\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title repeats=1
        Sequence
          Scalar String "Draft"
          Scalar String "Final"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_written_again_adds_its_items_and_any_other_value_adds_itself() {
    check(
        "\
---
tags: draft
tags: [review, parser]
labels: [review, parser]
labels: draft
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=tags repeats=1
        Sequence
          Scalar String "draft"
          Scalar String "review"
          Scalar String "parser"
      Pair key=labels repeats=1
        Sequence
          Scalar String "review"
          Scalar String "parser"
          Scalar String "draft"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_mapping_written_again_is_one_item_of_the_list() {
    check(
        "\
---
owner:
  name: Ada
owner:
  role: host
host: Ada
host:
  name: Grace
  role: host
people: [ada, grace]
people:
  - name: Linus
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner repeats=1
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
          Mapping
            Pair key=role
              Scalar String "host"
      Pair key=host repeats=1
        Sequence
          Scalar String "Ada"
          Mapping
            Pair key=name
              Scalar String "Grace"
            Pair key=role
              Scalar String "host"
      Pair key=people repeats=1
        Sequence
          Scalar String "ada"
          Scalar String "grace"
          Mapping
            Pair key=name
              Scalar String "Linus"
  MarkdownBlock
"#,
    );
}

#[test]
fn three_occurrences_give_one_list_in_file_order() {
    check(
        "---\na: 1\na: 2\na: 3\n---\n",
        "\
Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a repeats=2
        Sequence
          Scalar Integer 1
          Scalar Integer 2
          Scalar Integer 3
  MarkdownBlock
",
    );
}

#[test]
fn a_key_written_twice_in_a_nested_mapping_holds_a_list() {
    check(
        "\
---
owner:
  name: Ada
  name: Grace
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name repeats=1
            Sequence
              Scalar String "Ada"
              Scalar String "Grace"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_key_written_again_keeps_its_first_place() {
    check(
        "---\ntitle: A\nother: 1\ntitle: B\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title repeats=1
        Sequence
          Scalar String "A"
          Scalar String "B"
      Pair key=other
        Scalar Integer 1
  MarkdownBlock
"#,
    );
}

#[test]
fn each_later_occurrence_of_a_key_is_recorded_from_its_key_to_its_value() {
    let source = "---\ntitle: Draft\nonce: 1\ntitle: Final\ntitle:\n  - x\n---\n";
    let mapping = located_frontmatter(source);
    let title = &mapping.pairs["title"];
    assert_eq!(&source[title.span.clone()], "title: Draft");
    let repeats: Vec<&str> = title
        .repeats
        .iter()
        .map(|span| &source[span.clone()])
        .collect();
    assert_eq!(repeats, ["title: Final", "title:\n  - x"]);
    assert_eq!(mapping.pairs["once"].repeats, Vec::<Range<usize>>::new());
    assert_eq!(
        &source[mapping.span],
        "title: Draft\nonce: 1\ntitle: Final\ntitle:\n  - x"
    );
}

#[test]
fn a_literal_block_scalar_keeps_its_lines_with_each_chomping_and_an_indentation_digit() {
    check(
        "\
---
clip: |
  Two lines
  of text.

strip: |-
  no break

keep: |+
  kept

indented: |2
    four spaces
  two spaces
empty: |
kept_empty: |+

last: 1
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=clip
        Scalar String "Two lines\nof text.\n"
      Pair key=strip
        Scalar String "no break"
      Pair key=keep
        Scalar String "kept\n\n"
      Pair key=indented
        Scalar String "  four spaces\ntwo spaces\n"
      Pair key=empty
        Scalar String ""
      Pair key=kept_empty
        Scalar String "\n"
      Pair key=last
        Scalar Integer 1
  MarkdownBlock
"#,
    );
}

#[test]
fn a_folded_block_scalar_joins_its_lines_and_keeps_blank_and_more_indented_lines() {
    check(
        "\
---
text: >
  Folded into
  one line.

  A new paragraph,
    kept as written,
  and folded again.
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=text
        Scalar String "Folded into one line.\nA new paragraph,\n  kept as written,\nand folded again.\n"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_block_scalar_may_be_a_list_entry_or_a_value_inside_one() {
    check(
        "\
---
steps:
  - |
    first
  - name: x
    run: >-
      a
      b
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=steps
        Sequence
          Scalar String "first\n"
          Mapping
            Pair key=name
              Scalar String "x"
            Pair key=run
              Scalar String "a b"
  MarkdownBlock
"#,
    );
}

#[test]
fn flow_mappings_and_flow_lists_nest_and_may_span_several_lines() {
    check(
        r#"---
owner: {name: Ada, tags: [x, y]}
people: [{name: Ada}, {name: Grace, role: host}]
nested: {a: {b: [1, {c: 2}]}}
spread: {name: Ada,
  role: host}
list: [
  a,  # first
  b,
  ]
empty: {}
bare: {key, other: }
quoted: {"a b": 1, 'c': "d"}
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=tags
            Sequence
              Scalar String "x"
              Scalar String "y"
      Pair key=people
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
          Mapping
            Pair key=name
              Scalar String "Grace"
            Pair key=role
              Scalar String "host"
      Pair key=nested
        Mapping
          Pair key=a
            Mapping
              Pair key=b
                Sequence
                  Scalar Integer 1
                  Mapping
                    Pair key=c
                      Scalar Integer 2
      Pair key=spread
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=role
            Scalar String "host"
      Pair key=list
        Sequence
          Scalar String "a"
          Scalar String "b"
      Pair key=empty
        Mapping
      Pair key=bare
        Mapping
          Pair key=key
            Scalar Null
          Pair key=other
            Scalar Null
      Pair key=quoted
        Mapping
          Pair key=a b
            Scalar Integer 1
          Pair key=c
            Scalar String "d"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_non_finite_float_reads_as_its_text() {
    check(
        "\
---
up: .Inf
plus: +.INF
down: -.inf
nothing: .NaN
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=up
        Scalar String ".Inf"
      Pair key=plus
        Scalar String "+.INF"
      Pair key=down
        Scalar String "-.inf"
      Pair key=nothing
        Scalar String ".NaN"
  MarkdownBlock
"#,
    );
}

#[test]
fn digits_too_large_for_a_float_read_as_their_text_under_a_float_tag_too() {
    check(
        "---\nhuge: 1e999\nbelow: -1e999\ntagged: !!float 1e400\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=huge
        Scalar String "1e999"
      Pair key=below
        Scalar String "-1e999"
      Pair key=tagged
        Scalar String "1e400"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_tag_fixes_the_type_of_its_value() {
    check(
        r#"---
count: !!str 12
zip: !!str 01234
ratio: !!float 1
half: !!float 0.5
big: !!int "42"
done: !!bool true
nothing: !!null ~
when: !!timestamp 2026-10-10
at: !!timestamp 2026-10-10T07:32:00Z
verbatim: !<tag:yaml.org,2002:str> 12
listed: [!!str 1, !!int "2"]
!!str tagged key: !!str
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=count
        Scalar String "12"
      Pair key=zip
        Scalar String "01234"
      Pair key=ratio
        Scalar Float 1
      Pair key=half
        Scalar Float 0.5
      Pair key=big
        Scalar Integer 42
      Pair key=done
        Scalar Boolean true
      Pair key=nothing
        Scalar Null
      Pair key=when
        Scalar Date 2026-10-10
      Pair key=at
        Scalar OffsetDateTime 2026-10-10T07:32:00Z
      Pair key=verbatim
        Scalar String "12"
      Pair key=listed
        Sequence
          Scalar String "1"
          Scalar Integer 2
      Pair key=tagged key
        Scalar String ""
  MarkdownBlock
"#,
    );
}

#[test]
fn a_collection_tag_checks_its_value_whether_it_is_written_inline_or_as_a_block() {
    check(
        "\
---
owner: !!map {name: Ada}
tags: !!seq [a]
block: !!map
  name: Grace
items: !!seq
  - b
text: !!str |
  line
after: 1
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
      Pair key=tags
        Sequence
          Scalar String "a"
      Pair key=block
        Mapping
          Pair key=name
            Scalar String "Grace"
      Pair key=items
        Sequence
          Scalar String "b"
      Pair key=text
        Scalar String "line\n"
      Pair key=after
        Scalar Integer 1
  MarkdownBlock
"#,
    );
}

#[test]
fn an_unquoted_date_is_a_date_only_when_that_day_exists() {
    check(
        "\
---
due: 2028-02-29
late: 2026-02-30
never: 2026-04-31
century: 1900-02-29
millennium: 2000-02-29
at: 2026-02-30T10:00:00Z
local: 2026-10-10 07:32:00
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=due
        Scalar Date 2028-02-29
      Pair key=late
        Scalar String "2026-02-30"
      Pair key=never
        Scalar String "2026-04-31"
      Pair key=century
        Scalar String "1900-02-29"
      Pair key=millennium
        Scalar Date 2000-02-29
      Pair key=at
        Scalar String "2026-02-30T10:00:00Z"
      Pair key=local
        Scalar DateTime 2026-10-10T07:32:00
  MarkdownBlock
"#,
    );
}

#[test]
fn a_frontmatter_with_a_block_scalar_a_flow_mapping_a_tag_and_a_date_that_does_not_exist() {
    check(
        "\
---
summary: |
  Two lines
  of text.
owner: {name: Ada, tags: [x, y]}
count: !!str 12
due: 2026-02-30
---
# Notes
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=summary
        Scalar String "Two lines\nof text.\n"
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=tags
            Sequence
              Scalar String "x"
              Scalar String "y"
      Pair key=count
        Scalar String "12"
      Pair key=due
        Scalar String "2026-02-30"
  MarkdownBlock
    Section
      Heading
        Text "Notes"
"#,
    );
}

#[test]
fn a_flow_mapping_written_again_is_one_item_of_the_list() {
    check(
        "\
---
owner: {name: Ada}
owner: {role: host}
host: Ada
host: {name: Grace, role: host}
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner repeats=1
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
          Mapping
            Pair key=role
              Scalar String "host"
      Pair key=host repeats=1
        Sequence
          Scalar String "Ada"
          Mapping
            Pair key=name
              Scalar String "Grace"
            Pair key=role
              Scalar String "host"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_key_written_twice_in_a_flow_mapping_holds_a_list() {
    check(
        "---\nowner: {name: Ada, name: Grace}\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=owner
        Mapping
          Pair key=name repeats=1
            Sequence
              Scalar String "Ada"
              Scalar String "Grace"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_block_scalar_spans_from_its_indicator_to_its_last_text() {
    let whole = "---\ntext: |+\n  a\n  b\n\nflow: {k: [1]}\ntag: !!str x\n---\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            (
                "Frontmatter format=Yaml",
                "---\ntext: |+\n  a\n  b\n\nflow: {k: [1]}\ntag: !!str x\n---",
            ),
            (
                "Mapping",
                "text: |+\n  a\n  b\n\nflow: {k: [1]}\ntag: !!str x",
            ),
            ("Pair key=text", "text: |+\n  a\n  b"),
            (r#"Scalar String "a\nb\n\n""#, "|+\n  a\n  b"),
            ("Pair key=flow", "flow: {k: [1]}"),
            ("Mapping", "{k: [1]}"),
            ("Pair key=k", "k: [1]"),
            ("Sequence", "[1]"),
            ("Scalar Integer 1", "1"),
            ("Pair key=tag", "tag: !!str x"),
            (r#"Scalar String "x""#, "x"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn the_frontmatter_may_be_one_flow_mapping() {
    check(
        "---\n{\"a\": 1, b: [x]}\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=b
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\n# owner\n  {name: Ada,\n   role: host}  # flow\n\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=name
        Scalar String "Ada"
      Pair key=role
        Scalar String "host"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_plain_value_goes_on_over_the_lines_indented_more_than_its_key() {
    check(
        "\
---
title: a long
  title
folded: one

  two


  three
owner:
  name: Ada
    Lovelace
  role: host
items:
- first
 entry
- second # a comment ends it
signs: b - c
  [d] {e} \"f\" g:h
after: 1
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "a long title"
      Pair key=folded
        Scalar String "one\ntwo\n\nthree"
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada Lovelace"
          Pair key=role
            Scalar String "host"
      Pair key=items
        Sequence
          Scalar String "first entry"
          Scalar String "second"
      Pair key=signs
        Scalar String "b - c [d] {e} \"f\" g:h"
      Pair key=after
        Scalar Integer 1
  MarkdownBlock
"#,
    );
}

#[test]
fn a_plain_value_over_several_lines_is_typed_by_its_folded_text() {
    check(
        "\
---
at: 2026-10-10
  07:32:00
text: !!str 2026-10-10
  07:32:00
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=at
        Scalar DateTime 2026-10-10T07:32:00
      Pair key=text
        Scalar String "2026-10-10 07:32:00"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_quoted_value_goes_on_over_several_lines_and_folds_them() {
    check(
        r#"---
double: "a long
  title"
single: 'a long
  title'
blank: "one

  two"
spaces: 'it''s   
   folded  '
joined: "no \
  space, a\
  b"
escaped: "tab\t
  kept"
item:
- "first
  entry"
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=double
        Scalar String "a long title"
      Pair key=single
        Scalar String "a long title"
      Pair key=blank
        Scalar String "one\ntwo"
      Pair key=spaces
        Scalar String "it's folded  "
      Pair key=joined
        Scalar String "no space, ab"
      Pair key=escaped
        Scalar String "tab\t kept"
      Pair key=item
        Sequence
          Scalar String "first entry"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_pair_inside_a_flow_list_is_a_mapping_of_one_pair() {
    check(
        r#"---
t: [a: b, c]
forms: ["q": 1, k : v, empty: , bare:, http://x.com, a :b, !!str n: 2]
nested: [a: {b: [c: d]}]
---
"#,
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=t
        Sequence
          Mapping
            Pair key=a
              Scalar String "b"
          Scalar String "c"
      Pair key=forms
        Sequence
          Mapping
            Pair key=q
              Scalar Integer 1
          Mapping
            Pair key=k
              Scalar String "v"
          Mapping
            Pair key=empty
              Scalar Null
          Mapping
            Pair key=bare
              Scalar Null
          Scalar String "http://x.com"
          Scalar String "a :b"
          Mapping
            Pair key=n
              Scalar Integer 2
      Pair key=nested
        Sequence
          Mapping
            Pair key=a
              Mapping
                Pair key=b
                  Sequence
                    Mapping
                      Pair key=c
                        Scalar String "d"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_entry_with_nothing_on_its_line_holds_the_block_under_it() {
    check(
        "\
---
items:
-
  a: 1
- 
  - x
- # a comment
  b: 2
  c: 3
-
- last
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
          Sequence
            Scalar String "x"
          Mapping
            Pair key=b
              Scalar Integer 2
            Pair key=c
              Scalar Integer 3
          Scalar Null
          Scalar String "last"
  MarkdownBlock
"#,
    );
}

#[test]
fn one_trailing_comma_ends_a_flow_list() {
    check(
        "---\nt: [a, b,]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=t
        Sequence
          Scalar String "a"
          Scalar String "b"
  MarkdownBlock
"#,
    );
}

#[test]
fn the_yaml_forms_read_together() {
    check(
        "---\ntitle: a long\n  title\nitems:\n-\n  a: 1\nt: [a: b, c]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=title
        Scalar String "a long title"
      Pair key=items
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
      Pair key=t
        Sequence
          Mapping
            Pair key=a
              Scalar String "b"
          Scalar String "c"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_value_may_start_on_the_line_after_its_key_or_its_dash() {
    check(
        "\
---
a:
  b
quoted:
  \"b\"
flow:
  [x,
  y]
block:
  |
    b
long:
  b
  c
tagged:
  !!str 12
i:
-
  b
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "b"
      Pair key=quoted
        Scalar String "b"
      Pair key=flow
        Sequence
          Scalar String "x"
          Scalar String "y"
      Pair key=block
        Scalar String "b\n"
      Pair key=long
        Scalar String "b c"
      Pair key=tagged
        Scalar String "12"
      Pair key=i
        Sequence
          Scalar String "b"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_plain_value_inside_a_flow_collection_goes_on_over_the_lines_below() {
    check(
        "\
---
t: [a
  b]
m: {a: b
  c}
blank: [a b

  c, d]
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=t
        Sequence
          Scalar String "a b"
      Pair key=m
        Mapping
          Pair key=a
            Scalar String "b c"
      Pair key=blank
        Sequence
          Scalar String "a b\nc"
          Scalar String "d"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_entry_may_hold_a_mapping_whose_first_key_is_quoted() {
    check(
        "\
---
items:
- \"a\": 1
single:
- 'a': 1
  \"b\": 2
value:
- \"a: b\"
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
      Pair key=single
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
            Pair key=b
              Scalar Integer 2
      Pair key=value
        Sequence
          Scalar String "a: b"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_tag_that_ends_a_list_entry_takes_only_the_block_indented_under_the_dash() {
    check(
        "\
---
items:
- !!seq
  - x
- !!map
  k: v
key: !!seq
- a
---
",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Scalar String "x"
          Mapping
            Pair key=k
              Scalar String "v"
      Pair key=key
        Sequence
          Scalar String "a"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_comment_may_follow_a_tab() {
    check(
        "---\na: b\t# c\nq: \"x\"\t# c\n  \t# a comment line\nn: 1\t# c\nt: [x,\t# c\n  y]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "b"
      Pair key=q
        Scalar String "x"
      Pair key=n
        Scalar Integer 1
      Pair key=t
        Sequence
          Scalar String "x"
          Scalar String "y"
  MarkdownBlock
"#,
    );
}

#[test]
fn the_six_forms_read_together() {
    check(
        "---\na:\n  b\nt: [x\n  y]\nitems:\n- \"k\": 1\nc: d\t# note\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "b"
      Pair key=t
        Sequence
          Scalar String "x y"
      Pair key=items
        Sequence
          Mapping
            Pair key=k
              Scalar Integer 1
      Pair key=c
        Scalar String "d"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_value_on_the_line_after_its_key_spans_from_its_text_and_its_pair_from_the_key() {
    let whole = "---\na:\n  b\nt: [x\n  y]\n---\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("Frontmatter format=Yaml", "---\na:\n  b\nt: [x\n  y]\n---"),
            ("Mapping", "a:\n  b\nt: [x\n  y]"),
            ("Pair key=a", "a:\n  b"),
            (r#"Scalar String "b""#, "b"),
            ("Pair key=t", "t: [x\n  y]"),
            ("Sequence", "[x\n  y]"),
            (r#"Scalar String "x y""#, "x\n  y"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn a_value_over_several_lines_spans_from_its_first_text_to_its_last() {
    let whole = "---\nt: a\n\n  b # c\nq: \"x\n  y\"\nl: [k: v]\ni:\n-\n  - z\n---\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            (
                "Frontmatter format=Yaml",
                "---\nt: a\n\n  b # c\nq: \"x\n  y\"\nl: [k: v]\ni:\n-\n  - z\n---",
            ),
            (
                "Mapping",
                "t: a\n\n  b # c\nq: \"x\n  y\"\nl: [k: v]\ni:\n-\n  - z",
            ),
            ("Pair key=t", "t: a\n\n  b"),
            (r#"Scalar String "a\nb""#, "a\n\n  b"),
            ("Pair key=q", "q: \"x\n  y\""),
            (r#"Scalar String "x y""#, "\"x\n  y\""),
            ("Pair key=l", "l: [k: v]"),
            ("Sequence", "[k: v]"),
            ("Mapping", "k: v"),
            ("Pair key=k", "k: v"),
            (r#"Scalar String "v""#, "v"),
            ("Pair key=i", "i:\n-\n  - z"),
            ("Sequence", "-\n  - z"),
            ("Sequence", "- z"),
            (r#"Scalar String "z""#, "z"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn a_plain_value_may_start_with_a_question_mark_a_colon_or_a_dash_before_its_text() {
    check(
        "---\na: ?x\nb: :x\nc: -x\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "?x"
      Pair key=b
        Scalar String ":x"
      Pair key=c
        Scalar String "-x"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_entry_may_hold_a_mapping_whose_first_key_is_tagged() {
    check(
        "---\nitems:\n- !!str k: v\n  m: n\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=k
              Scalar String "v"
            Pair key=m
              Scalar String "n"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_entry_may_hold_a_list_that_starts_on_its_dash_line() {
    check(
        "---\nitems:\n- - a\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Scalar String "a"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- - a\n  - b\n- c\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Scalar String "a"
            Scalar String "b"
          Scalar String "c"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- - - a\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Sequence
              Scalar String "a"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n-   - a\n    - b\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Scalar String "a"
            Scalar String "b"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- - a: 1\n    b: 2\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Mapping
              Pair key=a
                Scalar Integer 1
              Pair key=b
                Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn spaces_may_sit_between_a_quoted_key_and_its_colon() {
    let one = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
";
    check("---\n\"a\" : 1\n---\n", one);
    check("---\n'a' :  1\n---\n", one);
    check("---\n\"a\" :\n  1\n---\n", one);
    check(
        "---\nitems:\n- \"a\" : 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_value_may_follow_a_quoted_key_colon_with_no_space_inside_a_flow_mapping() {
    check(
        "---\nt: {\"a\":1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=t
        Mapping
          Pair key=a
            Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn an_explicit_key_takes_its_value_from_the_colon_line_under_it() {
    check(
        "---\n? k\n: v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\n? k\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Scalar Null
  MarkdownBlock
",
    );
    check(
        "---\n? \"a b\"\n: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a b
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\n? k\n:\n  - x\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\na:\n  ? k\n  : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=k
            Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- ? k\n  : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=k
              Scalar String "v"
  MarkdownBlock
"#,
    );
}

#[test]
fn an_explicit_key_written_again_as_a_plain_key_holds_a_list_of_both_values() {
    check(
        "---\n? k\n: a\nk: b\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k repeats=1
        Sequence
          Scalar String "a"
          Scalar String "b"
  MarkdownBlock
"#,
    );
}

#[test]
fn an_explicit_key_pair_spans_from_its_question_mark_to_its_value() {
    let whole = "---\n? k\n: v\n---\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("Frontmatter format=Yaml", "---\n? k\n: v\n---"),
            ("Mapping", "? k\n: v"),
            ("Pair key=k", "? k\n: v"),
            (r#"Scalar String "v""#, "v"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn an_explicit_key_or_an_empty_key_inside_a_flow_collection_makes_a_pair() {
    check(
        "---\na: [? x]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=x
              Scalar Null
  MarkdownBlock
",
    );
    check(
        "---\na: {? x : 1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=x
            Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\na: [: x]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=
              Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\n{: x}\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=
        Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\na: [?]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=
              Scalar Null
  MarkdownBlock
",
    );
}

#[test]
fn a_question_mark_a_colon_or_a_dash_before_text_starts_a_plain_value_inside_a_flow_list() {
    check(
        "---\na: [-x, ?y, :z]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Scalar String "-x"
          Scalar String "?y"
          Scalar String ":z"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_tab_after_a_dash_separates_it_from_its_value() {
    check(
        "---\nitems:\n-\tx\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- -\ta\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Sequence
            Scalar String "a"
  MarkdownBlock
"#,
    );
}

#[test]
fn the_colon_line_of_an_explicit_key_may_hold_a_mapping_or_a_list() {
    check(
        "---\n? k\n: a: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Mapping
          Pair key=a
            Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\n? k\n: - x\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\n? k\n: a: 1\n  b: 2\nc: 3\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=k
        Mapping
          Pair key=a
            Scalar Integer 1
          Pair key=b
            Scalar Integer 2
      Pair key=c
        Scalar Integer 3
  MarkdownBlock
",
    );
}

#[test]
fn a_tab_after_the_indentation_of_a_continuation_a_flow_line_a_block_scalar_or_a_comment_stays() {
    check(
        "---\na: b\n  \tc\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "b c"
  MarkdownBlock
"#,
    );
    check(
        "---\nt: [a,\n \tb]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=t
        Sequence
          Scalar String "a"
          Scalar String "b"
  MarkdownBlock
"#,
    );
    check(
        "---\na: |\n  x\n  \ty\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "x\n\ty\n"
  MarkdownBlock
"#,
    );
    check(
        "---\na:\n \t# c\nb: 1\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Null
      Pair key=b
        Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_block_mapping_line_that_starts_with_a_colon_has_the_empty_key() {
    check(
        "---\n: v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=
        Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\na: 1\n: v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=
        Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=
              Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\n: v\n: w\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key= repeats=1
        Sequence
          Scalar String "v"
          Scalar String "w"
  MarkdownBlock
"#,
    );
}

#[test]
fn the_colon_of_a_flow_mapping_key_or_of_an_explicit_key_may_be_on_a_later_line() {
    let mapping = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=x
            Scalar Integer 1
  MarkdownBlock
";
    check("---\na: {x\n  : 1}\n---\n", mapping);
    check("---\na: {\"x\"\n  : 1}\n---\n", mapping);
    check("---\na: {? x\n  : 1}\n---\n", mapping);
    check(
        "---\na: [? x\n  : 1]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=x
              Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_tab_after_the_indentation_separates_a_flow_value() {
    check(
        "---\na:\n  \t\"x\"\nb:\n  \t[x]\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "x"
      Pair key=b
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\na:\n  \t{x: 1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=x
            Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\na:\n  \t!!str x\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n-\n  \t\"x\"\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Scalar String "x"
  MarkdownBlock
"#,
    );
    check(
        "---\n\t{a: 1}\n---\n",
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
fn a_plain_key_of_a_flow_mapping_goes_on_over_the_lines_below() {
    let mapping = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=x y
        Scalar Integer 1
  MarkdownBlock
";
    check("---\n{x\n  y: 1}\n---\n", mapping);
    check(
        "---\na: {x\n  y: 1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=x y
            Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\n{x\n  y\n  z: 1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=x y z
        Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_tag_followed_by_a_colon_is_a_tagged_empty_key() {
    let empty_key = r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=
        Scalar String "v"
  MarkdownBlock
"#;
    check("---\n!!str : v\n---\n", empty_key);
    check("---\n{!!str : v}\n---\n", empty_key);
    check(
        "---\na: 1\n!!str : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Scalar Integer 1
      Pair key=
        Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\na:\n  !!str : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=
            Scalar String "v"
  MarkdownBlock
"#,
    );
    check(
        "---\nitems:\n- !!str : v\n---\n",
        r#"Document
  Frontmatter format=Yaml
    Mapping
      Pair key=items
        Sequence
          Mapping
            Pair key=
              Scalar String "v"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_flow_key_may_be_a_tagged_empty_key_or_go_on_over_lines() {
    check(
        "---\n{!!str :}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=
        Scalar Null
  MarkdownBlock
",
    );
    check(
        "---\n{!!str :, a: 1}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=
        Scalar Null
      Pair key=a
        Scalar Integer 1
  MarkdownBlock
",
    );
    check(
        "---\na: {!!str :}\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=
            Scalar Null
  MarkdownBlock
",
    );
    check(
        "---\na: [!!str :]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=
              Scalar Null
  MarkdownBlock
",
    );
    check(
        "---\na: [? x\n  y : 1]\n---\n",
        "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Sequence
          Mapping
            Pair key=x y
              Scalar Integer 1
  MarkdownBlock
",
    );
    let quoted = "Document
  Frontmatter format=Yaml
    Mapping
      Pair key=a
        Mapping
          Pair key=x y
            Scalar Integer 1
  MarkdownBlock
";
    check("---\na: {\"x\n  y\": 1}\n---\n", quoted);
    check("---\na: {'x\n  y': 1}\n---\n", quoted);
}
