use chrono::{Datelike as _, Timelike as _};

use super::{broken, check, valid};
use crate::parser::ast::Value;

#[test]
fn every_scalar_type_is_read_from_how_the_value_is_written() {
    check(
        r#"+++
title = "Ship the parser"
path = 'C:\notes\parser'
priority = 2
big = 1_000_000
mask = 0xff
weight = 0.5
ratio = 1e-3
done = false
due = 2026-10-10
at = 07:32:00
start = 2026-10-10T07:32:00
sent = 2026-10-10T07:32:00.250-04:00
seen = 2026-10-10 09:00:00Z
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=title
        Scalar String "Ship the parser"
      Pair key=path
        Scalar String "C:\\notes\\parser"
      Pair key=priority
        Scalar Integer 2
      Pair key=big
        Scalar Integer 1000000
      Pair key=mask
        Scalar Integer 255
      Pair key=weight
        Scalar Float 0.5
      Pair key=ratio
        Scalar Float 0.001
      Pair key=done
        Scalar Boolean false
      Pair key=due
        Scalar Date 2026-10-10
      Pair key=at
        Scalar Time 07:32:00
      Pair key=start
        Scalar DateTime 2026-10-10T07:32:00
      Pair key=sent
        Scalar OffsetDateTime 2026-10-10T07:32:00.250000000-04:00
      Pair key=seen
        Scalar OffsetDateTime 2026-10-10T09:00:00Z
  MarkdownBlock
"#,
    );
}

#[test]
fn arrays_inline_tables_comments_and_quoted_keys() {
    check(
        r#"+++
# what this note is about
tags = ["parser", "design"]   # a list
matrix = [
  [1, 2],
  [3, 4],
]
owner = { name = "Ada", admin = true }
"quoted key" = "x"
empty = []
+++
Body text.
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=tags
        Sequence
          Scalar String "parser"
          Scalar String "design"
      Pair key=matrix
        Sequence
          Sequence
            Scalar Integer 1
            Scalar Integer 2
          Sequence
            Scalar Integer 3
            Scalar Integer 4
      Pair key=owner
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=admin
            Scalar Boolean true
      Pair key=quoted key
        Scalar String "x"
      Pair key=empty
        Sequence
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Body text."
"#,
    );
}

#[test]
fn a_basic_string_reads_its_escapes_and_a_literal_string_has_none() {
    check(
        r#"+++
basic = "tab\there \"quoted\" \u0041"
literal = 'tab\there'
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=basic
        Scalar String "tab\there \"quoted\" A"
      Pair key=literal
        Scalar String "tab\\there"
  MarkdownBlock
"#,
    );
}

#[test]
fn signed_numbers_other_bases_and_infinity() {
    check(
        "\
+++
negative = -17
positive = +5
binary = 0b101
octal = 0o17
exponent = 5e2
infinite = -inf
+++
",
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=negative
        Scalar Integer -17
      Pair key=positive
        Scalar Integer 5
      Pair key=binary
        Scalar Integer 5
      Pair key=octal
        Scalar Integer 15
      Pair key=exponent
        Scalar Float 500
      Pair key=infinite
        Scalar String "-inf"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_non_finite_float_reads_as_its_text() {
    check(
        "\
+++
up = +inf
nothing = nan
signed_nothing = -nan
+++
",
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=up
        Scalar String "+inf"
      Pair key=nothing
        Scalar String "nan"
      Pair key=signed_nothing
        Scalar String "-nan"
  MarkdownBlock
"#,
    );
}

#[test]
fn digits_too_large_for_a_float_read_as_their_text_as_written() {
    check(
        "+++\nhuge = 1e999\nbelow = -1e999\nunderscored = 1_0e400\n+++\n",
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=huge
        Scalar String "1e999"
      Pair key=below
        Scalar String "-1e999"
      Pair key=underscored
        Scalar String "1_0e400"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_lone_zero_underscores_between_digits_and_zeros_leading_an_exponent_are_read() {
    check(
        "\
+++
zero = 0
thousand = 1_000
zero_led_exponent = 1e02
fraction = 0.5
hexadecimal = 0xff_ff
exponent_underscore = 1e0_1
+++
",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=zero
        Scalar Integer 0
      Pair key=thousand
        Scalar Integer 1000
      Pair key=zero_led_exponent
        Scalar Float 100
      Pair key=fraction
        Scalar Float 0.5
      Pair key=hexadecimal
        Scalar Integer 65535
      Pair key=exponent_underscore
        Scalar Float 10
  MarkdownBlock
",
    );
}

#[test]
fn a_float_with_no_digit_before_its_dot_or_none_after_it_is_not_a_value() {
    for written in [".5", "5."] {
        let error = broken(&format!("+++\na = {written}\n+++\n")).error;
        assert_eq!(error.message, format!("`{written}` is not a value"));
    }
}

#[test]
fn a_key_written_twice_holds_a_list_of_both_values_and_keeps_its_first_place() {
    check(
        r#"+++
title = "Draft"
other = 1
title = "Final"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=title repeats=1
        Sequence
          Scalar String "Draft"
          Scalar String "Final"
      Pair key=other
        Scalar Integer 1
  MarkdownBlock
"#,
    );
}

#[test]
fn a_list_written_again_adds_its_items_and_any_other_value_adds_itself() {
    check(
        r#"+++
tags = "draft"
tags = ["review", "parser"]
labels = ["review", "parser"]
labels = "draft"
+++
"#,
        r#"Document
  Frontmatter format=Toml
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
fn a_table_written_again_is_one_item_of_the_list() {
    check(
        r#"+++
owner = { name = "Ada" }
owner = { role = "host" }
host = "Ada"
host = { name = "Grace", role = "host" }
people = ["ada", "grace"]
people = [{ name = "Linus" }]
+++
"#,
        r#"Document
  Frontmatter format=Toml
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
        "+++\na = 1\na = 2\na = 3\n+++\n",
        "\
Document
  Frontmatter format=Toml
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
fn a_key_written_twice_in_an_inline_table_holds_a_list() {
    check(
        r#"+++
owner = { name = "Ada", name = "Grace" }
+++
"#,
        r#"Document
  Frontmatter format=Toml
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
fn a_header_opens_a_table_that_holds_the_pairs_after_it() {
    check(
        r#"+++
title = "x"

[params]
author = "Ada"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=title
        Scalar String "x"
      Pair key=params
        Mapping
          Pair key=author
            Scalar String "Ada"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_dotted_header_makes_the_tables_on_its_path() {
    check(
        "+++\n[a.b]\nx = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a
        Mapping
          Pair key=b
            Mapping
              Pair key=x
                Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_header_adds_to_a_table_that_only_a_deeper_header_made() {
    check(
        r#"+++
[server.tls]
cert = "a.pem"

[server]
port = 8080
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=server repeats=1
        Mapping
          Pair key=tls
            Mapping
              Pair key=cert
                Scalar String "a.pem"
          Pair key=port
            Scalar Integer 8080
  MarkdownBlock
"#,
    );
}

#[test]
fn an_array_header_adds_a_mapping_and_a_deeper_header_opens_the_last_one() {
    check(
        r#"+++
[[authors]]
name = "Ada"

[[authors]]
name = "Grace"

[authors.site]
url = "grace.dev"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=authors repeats=2
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
          Mapping
            Pair key=name
              Scalar String "Grace"
            Pair key=site
              Mapping
                Pair key=url
                  Scalar String "grace.dev"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_table_header_written_twice_holds_a_list_of_both_tables() {
    check(
        "+++\n[params]\na = 1\n\n[params]\nb = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=params repeats=1
        Sequence
          Mapping
            Pair key=a
              Scalar Integer 1
          Mapping
            Pair key=b
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_after_a_dotted_key_or_an_inline_table_writes_the_table_again() {
    check(
        r#"+++
owner = { name = "Ada" }
site.name = "notes"

[owner]
role = "host"

[site]
lang = "en"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=owner repeats=1
        Sequence
          Mapping
            Pair key=name
              Scalar String "Ada"
          Mapping
            Pair key=role
              Scalar String "host"
      Pair key=site repeats=1
        Sequence
          Mapping
            Pair key=name
              Scalar String "notes"
          Mapping
            Pair key=lang
              Scalar String "en"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_dotted_key_writes_a_table_that_only_a_deeper_header_made() {
    check(
        r#"+++
[fruit.apple.texture]
smooth = true

[fruit]
apple.color = "red"

[fruit.apple]
taste = "sweet"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=fruit repeats=2
        Mapping
          Pair key=apple repeats=2
            Sequence
              Mapping
                Pair key=texture
                  Mapping
                    Pair key=smooth
                      Scalar Boolean true
                Pair key=color
                  Scalar String "red"
              Mapping
                Pair key=taste
                  Scalar String "sweet"
  MarkdownBlock
"#,
    );
}

#[test]
fn dotted_keys_add_to_one_table_and_a_quoted_dot_is_part_of_the_key() {
    check(
        r#"+++
owner.name = "Ada"
owner . role = "host"
"quoted.key" = 1
site."www.example.com".port = 80
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=owner repeats=1
        Mapping
          Pair key=name
            Scalar String "Ada"
          Pair key=role
            Scalar String "host"
      Pair key=quoted.key
        Scalar Integer 1
      Pair key=site
        Mapping
          Pair key=www.example.com
            Mapping
              Pair key=port
                Scalar Integer 80
  MarkdownBlock
"#,
    );
}

#[test]
fn a_dotted_key_through_an_inline_table_writes_the_key_again() {
    check(
        "+++\na = {b = 1}\na.c = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_through_an_inline_table_writes_the_key_again() {
    check(
        "+++\na = {b = 1}\n\n[a.x]\ny = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_header_through_an_array_written_with_equals_writes_the_key_again() {
    check(
        "+++\na = [{b = 1}]\n\n[a.x]\ny = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_header_through_an_inline_table_under_a_header_writes_the_key_again() {
    check(
        "+++\n[p]\na = {x = 1}\n\n[p.a.b]\ny = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=p repeats=1
        Mapping
          Pair key=a repeats=1
            Sequence
              Mapping
                Pair key=x
                  Scalar Integer 1
              Mapping
                Pair key=b
                  Mapping
                    Pair key=y
                      Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn an_array_header_through_an_inline_table_writes_the_key_again() {
    check(
        "+++\na = {b = 1}\n\n[[a.x]]\ny = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Sequence
                Mapping
                  Pair key=y
                    Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_header_adds_to_the_table_a_header_through_an_inline_table_made() {
    check(
        "+++\na = {b = 1}\n\n[a.x]\ny = 1\n\n[a]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=2
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_adds_to_the_table_a_header_through_an_array_written_with_equals_made() {
    check(
        "+++\na = [{b = 1}]\n\n[a.x]\ny = 1\n\n[a]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=2
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_second_header_after_a_header_through_an_inline_table_writes_the_key_again() {
    check(
        "+++\na = {b = 1}\n\n[a.x]\ny = 1\n\n[a]\nc = 2\n\n[a]\nd = 3\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=3
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
            Pair key=c
              Scalar Integer 2
          Mapping
            Pair key=d
              Scalar Integer 3
  MarkdownBlock
",
    );
}

#[test]
fn a_second_header_after_a_table_only_a_deeper_header_made_writes_the_key_again() {
    check(
        "+++\n[a.x]\ny = 1\n\n[a]\nb = 1\n\n[a]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=2
        Sequence
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn an_array_header_after_a_header_through_an_inline_table_adds_an_item() {
    check(
        "+++\na = {b = 1}\n\n[a.x]\ny = 1\n\n[[a]]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=2
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=x
              Mapping
                Pair key=y
                  Scalar Integer 1
          Mapping
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_after_an_inline_table_written_over_a_deeper_header_writes_the_key_again() {
    check(
        "+++\n[p.a.x]\ny = 1\n\n[p]\na = {z = 1}\n\n[p.a]\nw = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=p repeats=2
        Mapping
          Pair key=a repeats=2
            Sequence
              Mapping
                Pair key=x
                  Mapping
                    Pair key=y
                      Scalar Integer 1
              Mapping
                Pair key=z
                  Scalar Integer 1
              Mapping
                Pair key=w
                  Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_after_an_array_written_over_a_deeper_header_writes_the_key_again() {
    check(
        "+++\n[p.a.x]\ny = 1\n\n[p]\na = [{z = 1}]\n\n[p.a]\nw = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=p repeats=2
        Mapping
          Pair key=a repeats=2
            Sequence
              Mapping
                Pair key=x
                  Mapping
                    Pair key=y
                      Scalar Integer 1
              Mapping
                Pair key=z
                  Scalar Integer 1
              Mapping
                Pair key=w
                  Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_after_a_scalar_written_over_a_deeper_header_writes_the_key_again() {
    check(
        "+++\n[p.a.x]\ny = 1\n\n[p]\na = 1\n\n[p.a]\nw = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=p repeats=2
        Mapping
          Pair key=a repeats=2
            Sequence
              Mapping
                Pair key=x
                  Mapping
                    Pair key=y
                      Scalar Integer 1
              Scalar Integer 1
              Mapping
                Pair key=w
                  Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_opens_a_sub_table_of_a_table_a_header_made() {
    check(
        "+++\n[a]\nb = 1\n\n[a.x]\ny = 1\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Mapping
          Pair key=b
            Scalar Integer 1
          Pair key=x
            Mapping
              Pair key=y
                Scalar Integer 1
  MarkdownBlock
",
    );
}

#[test]
fn a_header_opens_a_sub_table_of_a_table_a_dotted_key_made() {
    check(
        r#"+++
[fruit]
apple.color = "red"

[fruit.apple.texture]
smooth = true
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=fruit repeats=1
        Mapping
          Pair key=apple repeats=1
            Mapping
              Pair key=color
                Scalar String "red"
              Pair key=texture
                Mapping
                  Pair key=smooth
                    Scalar Boolean true
  MarkdownBlock
"#,
    );
}

#[test]
fn a_header_after_an_inline_table_writes_the_key_again() {
    check(
        "+++\na = {b = 1}\n\n[a]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn dotted_keys_that_share_a_prefix_make_one_table() {
    check(
        "+++\na.b = 1\na.c = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Mapping
          Pair key=b
            Scalar Integer 1
          Pair key=c
            Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn a_header_written_twice_writes_the_key_again() {
    check(
        "+++\n[a]\nb = 1\n\n[a]\nc = 2\n+++\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=a repeats=1
        Sequence
          Mapping
            Pair key=b
              Scalar Integer 1
          Mapping
            Pair key=c
              Scalar Integer 2
  MarkdownBlock
",
    );
}

#[test]
fn dotted_keys_are_read_under_a_header_and_inside_an_inline_table() {
    check(
        r#"+++
[params]
social.github = "ada"
point = { x.value = 1, x.unit = "px" }
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=params
        Mapping
          Pair key=social
            Mapping
              Pair key=github
                Scalar String "ada"
          Pair key=point
            Mapping
              Pair key=x repeats=1
                Mapping
                  Pair key=value
                    Scalar Integer 1
                  Pair key=unit
                    Scalar String "px"
  MarkdownBlock
"#,
    );
}

#[test]
fn a_multi_line_basic_string_drops_its_first_newline_and_joins_lines_ending_in_a_backslash() {
    check(
        r#"+++
text = """
line one
line two"""
joined = """\
    The parser \
    reads a file."""
short = """
x \
  y"""
quotes = """a "quoted" word, two "" and a tab\t before one """"
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=text
        Scalar String "line one\nline two"
      Pair key=joined
        Scalar String "The parser reads a file."
      Pair key=short
        Scalar String "x y"
      Pair key=quotes
        Scalar String "a \"quoted\" word, two \"\" and a tab\t before one \""
  MarkdownBlock
"#,
    );
}

#[test]
fn a_multi_line_literal_string_keeps_its_backslashes() {
    check(
        r#"+++
path = '''C:\dir\n'''
pattern = '''
\d+ isn't a "string"'''
+++
"#,
        r#"Document
  Frontmatter format=Toml
    Mapping
      Pair key=path
        Scalar String "C:\\dir\\n"
      Pair key=pattern
        Scalar String "\\d+ isn't a \"string\""
  MarkdownBlock
"#,
    );
}

/// The value under `key` in the frontmatter of `source`.
fn scalar(source: &str, key: &str) -> Option<Value> {
    valid(source).frontmatter?.swap_remove(key)
}

#[test]
fn a_date_time_with_an_offset_holds_it_and_one_without_has_none() {
    let sent = scalar("+++\nts = 2026-10-10T07:32:00-04:00\n+++\n", "ts");
    assert!(matches!(sent, Some(Value::OffsetDateTime(ts))
        if ts.offset().local_minus_utc() == -4_i32 * 3_600_i32
            && (ts.year(), ts.hour()) == (2_026_i32, 7_u32)));

    let start = scalar("+++\nts = 2026-10-10T07:32:00\n+++\n", "ts");
    assert!(matches!(start, Some(Value::DateTime(ts))
        if (ts.day(), ts.minute()) == (10_u32, 32_u32)));

    let at = scalar("+++\nat = 07:32:00\n+++\n", "at");
    assert!(matches!(at, Some(Value::Time(time))
        if (time.hour(), time.minute(), time.second()) == (7_u32, 32_u32, 0_u32)));
}

#[test]
fn a_date_is_a_date_only_when_that_day_exists() {
    check(
        "\
+++
leap = 2028-02-29
millennium = 2000-02-29
+++
",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=leap
        Scalar Date 2028-02-29
      Pair key=millennium
        Scalar Date 2000-02-29
  MarkdownBlock
",
    );
}

#[test]
fn every_separator_and_offset_form_of_a_date_time_is_read() {
    check(
        "\
+++
upper = 2026-10-10T07:32:00Z
lower = 2026-10-10t07:32:00z
spaced = 2026-10-10 07:32:00+05:30
zero = 2026-10-10T07:32:00-00:00
local = 2026-10-10 07:32:00.5
leap = 23:59:60
leap_fraction = 23:59:60.25
leap_date_time = 2026-12-31T18:59:60-05:00
+++
",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=upper
        Scalar OffsetDateTime 2026-10-10T07:32:00Z
      Pair key=lower
        Scalar OffsetDateTime 2026-10-10T07:32:00Z
      Pair key=spaced
        Scalar OffsetDateTime 2026-10-10T07:32:00+05:30
      Pair key=zero
        Scalar OffsetDateTime 2026-10-10T07:32:00Z
      Pair key=local
        Scalar DateTime 2026-10-10T07:32:00.500000000
      Pair key=leap
        Scalar Time 23:59:60
      Pair key=leap_fraction
        Scalar Time 23:59:60.250000000
      Pair key=leap_date_time
        Scalar OffsetDateTime 2026-12-31T18:59:60-05:00
  MarkdownBlock
",
    );
}
