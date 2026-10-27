use core::slice;

use super::{broken, on_two_megabytes, valid};
use crate::parser::ast::{FrontmatterFormat, Mapping, RawDocument, Value};
use crate::parser::error::ParseError;
use crate::parser::extension::Plain;
use crate::parser::frontmatter::VALUE_CAP;
use crate::parser::{ParseOptions, from_json, parse, parse_with, to_json};
use crate::writer::{WriteOptions, write};

/// The error a strict parse of `source` fails with. A parse that is not
/// strict lists it alone and holds its message in an invalid frontmatter when
/// no entry of the frontmatter reads, and else drops an entry first with its
/// message.
fn strict_error(source: &str) -> ParseError {
    let error = parse_with::<Plain>(source, &ParseOptions::strict()).unwrap_err();
    let parsed = parse(source);
    match parsed.document {
        RawDocument::Invalid(invalid) => {
            assert_eq!(invalid.frontmatter.error, error.message, "{source:?}");
            assert_eq!(parsed.dropped, slice::from_ref(&error), "{source:?}");
            assert_eq!(broken(source).error, error, "{source:?}");
        }
        RawDocument::Valid(_) => {
            let first = parsed.dropped.first().map(|drop| &drop.message);
            assert_eq!(first, Some(&error.message), "{source:?}");
        }
    }
    error
}

/// The message a strict parse of `source` fails with and the source text the
/// error points at.
fn failure(source: &str) -> (String, &str) {
    let error = strict_error(source);
    (error.message, &source[error.span])
}

/// The message a strict parse of `source` fails with and the rest of the
/// line the error starts on.
fn refusal(source: &str) -> (String, &str) {
    let error = strict_error(source);
    let rest = &source[error.span.start..];
    (error.message, rest.lines().next().unwrap_or(rest))
}

#[test]
fn the_error_names_the_byte_it_points_at() {
    let error = broken("+++\ndue = tomorrow\n+++\n").error;
    assert_eq!(error.to_string(), "`tomorrow` is not a value (at byte 10)");
}

#[test]
fn a_toml_value_that_is_no_type_is_refused() {
    let (message, at) = failure("+++\ndue = tomorrow\n+++\n");
    assert_eq!(message, "`tomorrow` is not a value");
    assert_eq!(at, "tomorrow");
}

#[test]
fn a_toml_string_left_open_is_refused() {
    let (message, _) = failure("+++\ntitle = \"open\n+++\n");
    assert_eq!(message, "the string is not closed");
}

#[test]
fn a_toml_table_header_or_multi_line_string_left_open_is_refused() {
    let (header, _) = failure("+++\n[owner\nname = \"Ada\"\n+++\n");
    assert_eq!(header, "expected `]` to close the table header");

    let (array, _) = failure("+++\n[[owner]\nname = \"Ada\"\n+++\n");
    assert_eq!(array, "expected `]` to close the table header");

    let (basic, _) = failure("+++\ntext = \"\"\"\nlong\n+++\n");
    assert_eq!(basic, "the string is not closed");

    let (literal, _) = failure("+++\ntext = '''\nlong\n+++\n");
    assert_eq!(literal, "the string is not closed");
}

#[test]
fn a_toml_inline_table_stays_on_one_line() {
    let (message, _) = failure("+++\nt = { a = 1,\n  b = 2 }\n+++\n");
    assert_eq!(message, "expected a key");
}

/// The message an error names, with what it points at.
fn named(message: &str, at: &str) -> (String, String) {
    (message.to_owned(), at.to_owned())
}

/// [`failure`] with owned parts, to compare with [`named`].
fn failed(source: &str) -> (String, String) {
    let (message, at) = failure(source);
    (message, at.to_owned())
}

#[test]
fn a_dropped_tag_does_not_hide_text_yaml_does_not_allow_before_its_value() {
    assert_eq!(
        failed("---\n!point @x: 1\n---\n"),
        named("a plain key cannot start with `@`; quote it", "@")
    );
    assert_eq!(
        failed("---\na: !point\n  \t- x\n---\n"),
        named("a tab cannot indent a line; use spaces", "\t")
    );
}

#[test]
fn a_value_that_does_not_fit_its_tag_is_refused() {
    assert_eq!(
        failed("---\nn: !!int abc\n---\n"),
        named("`abc` is not a value of the tag `!!int`", "abc")
    );
    assert_eq!(
        failed("---\nratio: !!float 0x1F\n---\n"),
        named("`0x1F` is not a value of the tag `!!float`", "0x1F")
    );
    assert_eq!(
        failed("---\nx: !!float .inf\n---\n"),
        named("`.inf` is not a value of the tag `!!float`", ".inf")
    );
    assert_eq!(
        failed("---\nx: !!float .nan\n---\n"),
        named("`.nan` is not a value of the tag `!!float`", ".nan")
    );
    assert_eq!(
        failed("---\ndone: !!bool yes\n---\n"),
        named("`yes` is not a value of the tag `!!bool`", "yes")
    );
    assert_eq!(
        failed("---\nnothing: !!null \"x\"\n---\n"),
        named("`x` is not a value of the tag `!!null`", "\"x\"")
    );
    assert_eq!(
        failed("---\ndue: !!timestamp 2026-02-30\n---\n"),
        named(
            "`2026-02-30` is not a value of the tag `!!timestamp`",
            "2026-02-30"
        )
    );
    assert_eq!(
        failed("---\nowner: !!map Ada\n---\n"),
        named("`Ada` is not a value of the tag `!!map`", "Ada")
    );
    assert_eq!(
        failed("---\ntags: !!seq {a: 1}\n---\n"),
        named("a mapping is not a value of the tag `!!seq`", "{a: 1}")
    );
    assert_eq!(
        failed("---\ntitle: !!str\n  - a\n---\n"),
        named("a list is not a value of the tag `!!str`", "- a")
    );
    assert_eq!(
        failed("---\n!!int one: 1\n---\n"),
        named("`one` is not a value of the tag `!!int`", "one")
    );
}

#[test]
fn a_tag_with_no_value_after_it_or_under_it_is_checked_as_an_empty_value() {
    let empty = |name: &str| {
        named(
            &format!("an empty value is not a value of the tag `{name}`"),
            name,
        )
    };
    assert_eq!(failed("---\nitems:\n- !!seq\n- x\n---\n"), empty("!!seq"));
    assert_eq!(
        failed("---\nitems:\n-\n  !!seq\n- x\n---\n"),
        empty("!!seq")
    );
    assert_eq!(
        failed("---\nowner: !!map\nname: Ada\n---\n"),
        empty("!!map")
    );
    assert_eq!(failed("---\nn: [!!int ]\n---\n"), empty("!!int"));
    assert_eq!(failed("---\n!!int : v\n---\n"), empty("!!int"));
}

#[test]
fn a_line_of_a_flow_collection_must_be_indented_more_than_its_key() {
    let message = "a line of a flow collection must be indented more than its key";
    let refused = |at: &'static str| (message.to_owned(), at);
    assert_eq!(refusal("---\nflow: [a,\nb,\nc]\n---\n"), refused("b,"));
    assert_eq!(refusal("---\nflow: {a: 1,\nb: 2}\n---\n"), refused("b: 2}"));
    assert_eq!(refusal("---\nlist: [\n  a,\n]\n---\n"), refused("]"));
    assert_eq!(refusal("---\nt: [a\nb]\n---\n"), refused("b]"));
    assert_eq!(refusal("---\nt: [a, [b,\nc]]\n---\n"), refused("c]]"));
    assert_eq!(refusal("---\nitems:\n- [a,\nb]\n---\n"), refused("b]"));
    assert_eq!(refusal("---\nowner:\n  t: [a,\n  b]\n---\n"), refused("b]"));
    assert_eq!(refusal("---\nflow: {a\nb: 1}\n---\n"), refused("b: 1}"));
}

#[test]
fn an_alias_with_a_tag_or_an_anchor_is_refused_at_the_property() {
    let refused = |at: &str| named("an alias holds no tag or anchor", at);
    assert_eq!(failed("---\na: &x 1\nb: !!str *x\n---\n"), refused("!!str"));
    assert_eq!(failed("---\na: !!str *a\n---\n"), refused("!!str"));
    assert_eq!(failed("---\na: &x 1\nb: &y *x\n---\n"), refused("&y"));
    assert_eq!(failed("---\na: &x 1\nb: [&y *x]\n---\n"), refused("&y"));
    assert_eq!(failed("---\na: &x 1\nb: &y\n  *x\n---\n"), refused("&y"));
    assert_eq!(
        failed("---\na: &x 1\n!!str *x : 2\n---\n"),
        refused("!!str")
    );
}

#[test]
fn a_second_anchor_on_a_value_or_a_key_is_refused_where_it_starts() {
    for source in [
        "---\na: &x &y 1\n---\n",
        "---\n&x &y a: 1\n---\n",
        "---\na: [&x &y 1]\n---\n",
        "---\na: &x\n  &y 1\n---\n",
    ] {
        let error = broken(source).error;
        assert_eq!(error.message, "a value has at most one anchor");
        let at = source.rfind('&').unwrap();
        assert_eq!(error.span, at..at + 1);
    }
}

#[test]
fn an_anchor_or_an_alias_with_no_name_is_refused() {
    assert_eq!(
        failed("---\na: & 1\n---\n"),
        named("expected a name after `&`", "&")
    );
    assert_eq!(
        failed("---\na: [*]\n---\n"),
        named("expected a name after `*`", "*")
    );
}

#[test]
fn a_plain_value_cannot_hold_a_colon_and_a_space() {
    assert_eq!(
        refusal("---\ntitle: Notes: draft\n---\n"),
        (
            "a plain value cannot hold `: `; quote it".to_owned(),
            "Notes: draft"
        )
    );
    assert_eq!(
        refusal("---\ntags: [a: b: c]\n---\n"),
        (
            "a plain value cannot hold `: `; quote it".to_owned(),
            "b: c]"
        )
    );
}

#[test]
fn a_plain_value_cannot_hold_a_colon_and_a_tab() {
    let tab = "---\na: b:\tc\n---\n";
    assert_eq!(
        refusal(tab),
        (
            "a plain value cannot hold `: `; quote it".to_owned(),
            "b:\tc"
        )
    );
    assert_eq!(broken(tab).error, broken("---\na: b: c\n---\n").error);
}

#[test]
fn a_line_whose_text_before_its_comment_holds_no_key_is_refused_in_a_mapping() {
    assert_eq!(
        refusal("---\na: 1\nabc # c: d\n---\n"),
        ("expected `key: value`".to_owned(), "abc # c: d")
    );
}

#[test]
fn an_empty_entry_of_a_flow_collection_is_refused_at_its_comma() {
    let refused = |source: &str| {
        let error = broken(source).error;
        (error.message, error.span)
    };
    let before_comma = |at: usize| ("expected a value before `,`".to_owned(), at..at + 1);
    assert_eq!(refused("---\nt: [a,,b]\n---\n"), before_comma(10));
    assert_eq!(refused("---\nt: [,]\n---\n"), before_comma(8));
    assert_eq!(refused("---\nt: [a,,]\n---\n"), before_comma(10));
    assert_eq!(refused("---\nt: {a,,b}\n---\n"), before_comma(10));
    assert_eq!(refused("---\nt: {,}\n---\n"), before_comma(8));
}

#[test]
fn a_frontmatter_with_more_after_its_one_value_is_refused() {
    let not_a_mapping = |at: &str| named("the frontmatter must be a mapping", at);
    assert_eq!(failed("---\n[a, b]\nc: 1\n---\n"), not_a_mapping("[a, b]"));
    assert_eq!(failed("---\n[a, b] c\n---\n"), not_a_mapping("[a, b]"));
    assert_eq!(
        failed("---\n[!point a]\nc: 1\n---\n"),
        not_a_mapping("[!point a]\nc: 1")
    );
    assert_eq!(
        refusal("---\n{a: 1}\nb: 2\n---\n"),
        (
            "expected the end of the frontmatter after the mapping".to_owned(),
            "b: 2"
        )
    );
}

#[test]
fn a_frontmatter_that_holds_one_value_or_one_list_that_does_not_read_is_not_a_mapping() {
    let not_a_mapping = |at: &str| named("the frontmatter must be a mapping", at);
    for (source, at) in [
        ("---\n[unclosed\n---\n", "[unclosed"),
        ("---\n!!int abc\n---\n", "!!int abc"),
        ("---\n- !!int abc\n---\n", "- !!int abc"),
        ("---\n[!!int abc]\n---\n", "[!!int abc]"),
        ("---\n!!int abc\n\n# b: 1\n\n---\n", "!!int abc\n\n# b: 1"),
    ] {
        assert_eq!(failed(source), not_a_mapping(at));
    }
}

#[test]
fn a_line_inside_a_quoted_string_must_be_indented_more_than_its_key() {
    let refused = (
        "a line inside a quoted string must be indented more than its key or `-`".to_owned(),
        "b\"",
    );
    assert_eq!(refusal("---\nt: \"a\nb\"\n---\n"), refused);
    assert_eq!(refusal("---\nowner:\n  t: 'a\n  b'\n---\n").0, refused.0);
    assert_eq!(refusal("---\nitems:\n- \"a\nb\"\n---\n"), refused);
    assert_eq!(refusal("---\nt: [\"a\nb\"]\n---\n").0, refused.0);
}

#[test]
fn a_yaml_flow_collection_left_open_is_refused() {
    let (list, _) = failure("---\ntags: [a, b\n---\n");
    assert_eq!(list, "expected `,` or `]` in the list");

    let (mapping, _) = failure("---\nowner: {name: Ada\n---\n");
    assert_eq!(mapping, "expected `,` or `}` in the mapping");
}

#[test]
fn a_pair_whose_key_is_a_list_or_a_mapping_is_refused_as_its_value_is_in_a_pair_that_is_kept() {
    let at_end = |source: &str| {
        let end = source.find("[unclosed\n").unwrap() + "[unclosed\n".len();
        ("expected `,` or `]` in the list".to_owned(), end..end)
    };
    let refused = |source: &str| {
        let error = strict_error(source);
        (error.message, error.span)
    };
    for source in [
        "---\na: [unclosed\n---\n",
        "---\n[x]: [unclosed\n---\n",
        "---\na: 1\n{x: 2}: [unclosed\n---\n",
        "---\n? [x]\n: [unclosed\n---\n",
        "---\nitems:\n- [x]: [unclosed\n---\n",
    ] {
        assert_eq!(refused(source), at_end(source));
    }
    assert_eq!(
        failed("---\n!!str [x]: y\n---\n"),
        named("a list is not a value of the tag `!!str`", "[x]")
    );
    assert_eq!(
        failed("---\na: {[!!int x]: 1}\n---\n"),
        named("`x` is not a value of the tag `!!int`", "x")
    );
}

#[test]
fn a_block_scalar_with_a_bad_header_or_a_deeper_blank_line_is_refused() {
    let (header, _) = failure("---\ntext: | words\n  line\n---\n");
    assert_eq!(header, "expected the end of the line after the value");

    assert_eq!(
        refusal("---\ntext: |\n    \n  line\n---\n"),
        (
            "a blank line before the text of a block scalar is indented more than the text"
                .to_owned(),
            "  line"
        )
    );
}

#[test]
fn a_toml_date_that_does_not_exist_is_refused() {
    assert_eq!(
        failed("+++\ndue = 2026-02-30\n+++\n"),
        named("`2026-02-30` is not a value", "2026-02-30")
    );
    assert_eq!(
        failed("+++\nat = 2026-04-31 10:00:00\n+++\n"),
        named("`2026-04-31` is not a value", "2026-04-31")
    );
    assert_eq!(
        failed("+++\ncentury = 1900-02-29\n+++\n"),
        named("`1900-02-29` is not a value", "1900-02-29")
    );
}

#[test]
fn a_toml_time_or_offset_out_of_range_is_refused() {
    assert_eq!(
        failed("+++\nat = 24:00:00\n+++\n"),
        named("`24:00:00` is not a value", "24:00:00")
    );
    assert_eq!(
        failed("+++\nat = 2026-10-10T07:32:00+24:00\n+++\n"),
        named(
            "`2026-10-10T07:32:00+24:00` is not a value",
            "2026-10-10T07:32:00+24:00"
        )
    );
}

#[test]
fn a_toml_number_with_a_leading_zero_or_a_misplaced_underscore_is_refused() {
    for written in [
        "01",
        "1__0",
        "_1",
        "1_",
        "0x_ff",
        "01.5",
        "099999999999999999999",
        "0x_ffffffffffffffffff",
    ] {
        assert_eq!(
            failed(&format!("+++\na = {written}\n+++\n")),
            named(&format!("`{written}` is not a value"), written)
        );
    }
}

#[test]
fn yaml_indentation_that_matches_no_block_is_refused() {
    let (deeper, _) = failure("---\na: 1\n  b: 2\n---\n");
    assert_eq!(deeper, "this line is indented more than the keys above it");

    let (shallower, _) = failure("---\n  a: 1\nb: 2\n---\n");
    assert_eq!(shallower, "this line is indented less than the first key");
}

#[test]
fn a_plain_value_cannot_start_with_an_indicator() {
    let refused = |character: &str| {
        named(
            &format!("a plain value cannot start with `{character}`; quote it"),
            character,
        )
    };
    assert_eq!(failed("---\na: @x\n---\n"), refused("@"));
    assert_eq!(failed("---\na: `x\n---\n"), refused("`"));
    assert_eq!(failed("---\na: %x\n---\n"), refused("%"));
    assert_eq!(failed("---\na: ,x\n---\n"), refused(","));
    assert_eq!(failed("---\na: ]x\n---\n"), refused("]"));
    assert_eq!(failed("---\na: }x\n---\n"), refused("}"));
    assert_eq!(failed("---\na:\n  @x\n---\n"), refused("@"));
    assert_eq!(failed("---\nitems:\n- @x\n---\n"), refused("@"));
    assert_eq!(failed("---\na: !!str @x\n---\n"), refused("@"));
    assert_eq!(failed("---\nt: [@x]\n---\n"), refused("@"));
    assert_eq!(failed("---\nt: [a, `b]\n---\n"), refused("`"));
    assert_eq!(
        refusal("---\na: -\n---\n"),
        (
            "a plain value cannot start with `-`; quote it".to_owned(),
            "-"
        )
    );
}

#[test]
fn a_plain_key_cannot_start_with_an_indicator() {
    let refused = |character: &str| {
        named(
            &format!("a plain key cannot start with `{character}`; quote it"),
            character,
        )
    };
    assert_eq!(failed("---\n@a: 1\n---\n"), refused("@"));
    assert_eq!(failed("---\n`a: 1\n---\n"), refused("`"));
    assert_eq!(failed("---\na: {@y: 1}\n---\n"), refused("@"));
}

#[test]
fn a_plain_value_inside_a_flow_collection_cannot_start_with_a_block_indicator_or_a_lone_dash() {
    let refused = |character: &str| {
        named(
            &format!("a plain value cannot start with `{character}`; quote it"),
            character,
        )
    };
    assert_eq!(failed("---\nt: [|x]\n---\n"), refused("|"));
    assert_eq!(failed("---\nt: [>x]\n---\n"), refused(">"));
    assert_eq!(failed("---\na: [-]\n---\n"), refused("-"));
}

#[test]
fn a_tab_after_a_dash_does_not_indent_a_list_or_a_mapping_on_its_line() {
    assert_eq!(
        refusal("---\nitems:\n-\t- a\n---\n"),
        (
            "a plain value cannot start with `-` and a space; quote it".to_owned(),
            "- a"
        )
    );
    assert_eq!(
        refusal("---\nitems:\n-\ta: 1\n---\n").0,
        "a plain value cannot hold `: `; quote it"
    );
}

#[test]
fn a_plain_value_cannot_start_with_a_dash_a_question_mark_or_a_colon_and_a_space() {
    let refused = |character: &str, at: &'static str| {
        (
            format!("a plain value cannot start with `{character}` and a space; quote it"),
            at,
        )
    };
    assert_eq!(refusal("---\na: - x\n---\n"), refused("-", "- x"));
    assert_eq!(refusal("---\na: ? k\n---\n"), refused("?", "? k"));
    assert_eq!(refusal("---\na: : x\n---\n"), refused(":", ": x"));
}

#[test]
fn a_block_key_needs_a_space_after_its_colon() {
    let refused = ("expected a space after the `:`".to_owned(), "1");
    assert_eq!(refusal("---\n\"a\":1\n---\n"), refused);
    assert_eq!(refusal("---\nitems:\n- \"a\":1\n---\n"), refused);
}

#[test]
fn the_colon_line_of_an_explicit_key_indented_more_than_its_question_mark_is_refused() {
    assert_eq!(
        refusal("---\n? k\n  : v\n---\n"),
        (
            "this line is indented more than the keys above it".to_owned(),
            "  : v"
        )
    );
}

#[test]
fn a_block_key_cannot_start_with_a_block_scalar_indicator() {
    let refused = |character: &str| {
        named(
            &format!("a plain key cannot start with `{character}`; quote it"),
            character,
        )
    };
    assert_eq!(failed("---\n|x: 1\n---\n"), refused("|"));
    assert_eq!(failed("---\na: 1\n>x: 2\n---\n"), refused(">"));
    assert_eq!(failed("---\na:\n  b: 1\n  >x: 2\n---\n"), refused(">"));
}

#[test]
fn a_tab_before_a_key_or_a_list_entry_is_refused_at_the_tab() {
    let refused = |source: &str| {
        let error = strict_error(source);
        (error.message, error.span)
    };
    let at_tab = |at: usize| {
        (
            "a tab cannot indent a line; use spaces".to_owned(),
            at..at + 1,
        )
    };
    assert_eq!(refused("---\na:\n\tb: 1\n---\n"), at_tab(7));
    assert_eq!(refused("---\n\ta: 1\n---\n"), at_tab(4));
    assert_eq!(refused("---\na:\n\t- b\n---\n"), at_tab(7));
    assert_eq!(refused("---\na:\n  \tb: 1\n---\n"), at_tab(9));
    assert_eq!(refused("---\na:\n  \t- x\n---\n"), at_tab(9));
    assert_eq!(refused("---\n\t- a\n---\n"), at_tab(4));
    assert_eq!(refused("---\n\t? k\n---\n"), at_tab(4));
    assert_eq!(refused("---\n\t: v\n---\n"), at_tab(4));
    assert_eq!(refused("---\na:\n  \t!!str : v\n---\n"), at_tab(9));
    assert_eq!(refused("---\na:\n  \t[x]: y\n---\n"), at_tab(9));
}

#[test]
fn an_implicit_key_inside_a_flow_list_keeps_its_colon_on_its_line() {
    assert_eq!(
        refusal("---\na: [x\n  : 1]\n---\n"),
        ("expected `,` or `]` in the list".to_owned(), ": 1]")
    );
}

#[test]
fn the_key_of_a_pair_inside_a_flow_list_stays_on_one_line() {
    let refused = |at: &'static str| ("expected `,` or `]` in the list".to_owned(), at);
    assert_eq!(
        failed("---\n[x\n  y: 1]\n---\n"),
        named("the frontmatter must be a mapping", "[x\n  y: 1]")
    );
    assert_eq!(refusal("---\na: [x\n  y: 1]\n---\n"), refused("y: 1]"));
}

/// The frontmatter of each shape of nesting the readers count, its values
/// `depth` deep.
fn nested_frontmatter(depth: usize) -> [String; 6] {
    let lists = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let tables = format!("{}1{}", "{b = ".repeat(depth), "}".repeat(depth));
    let mut keys = String::new();
    for level in 0..depth {
        keys.push_str(&"  ".repeat(level));
        keys.push_str("a:\n");
    }
    keys.push_str(&"  ".repeat(depth));
    keys.push_str("b: 1\n");
    [
        format!("+++\na = {lists}\n+++\n"),
        format!("+++\na = {tables}\n+++\n"),
        format!("+++\n{}b = 1\n+++\n", "a.".repeat(depth)),
        format!("+++\n[{}b]\nc = 1\n+++\n", "a.".repeat(depth - 1)),
        format!("---\na: {lists}\n---\n"),
        format!("---\n{keys}---\n"),
    ]
}

/// How deep the values of `mapping` nest: the mappings and lists around the
/// deepest one, and itself.
fn value_depth(mapping: &Mapping) -> usize {
    let mut deepest = 0;
    let mut open: Vec<(usize, &Value)> = mapping.values().map(|value| (1, value)).collect();
    while let Some((depth, value)) = open.pop() {
        if let Value::Mapping(inner) = value {
            deepest = deepest.max(depth);
            open.extend(inner.values().map(|item| (depth + 1, item)));
        }
        if let Value::Sequence(items) = value {
            deepest = deepest.max(depth);
            open.extend(items.iter().map(|item| (depth + 1, item)));
        }
    }
    deepest
}

/// The depth of the values of the frontmatter of `document`, or `None` when
/// no entry of it reads.
fn frontmatter_depth(document: &RawDocument) -> Option<usize> {
    match document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => Some(valid.frontmatter.as_ref().map_or(0, value_depth)),
    }
}

#[test]
fn frontmatter_nested_to_the_value_cap_goes_through_every_operation() {
    on_two_megabytes(|| {
        for source in nested_frontmatter(VALUE_CAP) {
            let document = parse(&source).document;
            assert_eq!(frontmatter_depth(&document), Some(VALUE_CAP));
            for format in [FrontmatterFormat::Toml, FrontmatterFormat::Yaml] {
                let options = WriteOptions {
                    frontmatter: format,
                };
                let written = write(&document, &options).unwrap();
                let depth = frontmatter_depth(&parse(&written).document);
                assert_eq!(depth, Some(VALUE_CAP));
            }
            let copy = document.clone();
            assert_eq!(copy, document);
            let printed = format!("{document:?}");
            assert!(printed.starts_with("Valid(Document {"));
            drop(copy);
            let json = to_json(&document).unwrap();
            let back: RawDocument = from_json(&json).unwrap();
            assert_eq!(to_json(&back).unwrap(), json);
            drop(back);
            drop(document);
        }
    });
}

/// Reads `source`, checks that its frontmatter nests `depth` deep, `None`
/// when no entry of it reads, and writes it in YAML and in TOML, each written
/// text reading back to that depth.
fn reads_and_writes_at_depth(source: &str, depth: Option<usize>) {
    let document = parse(source).document;
    assert_eq!(frontmatter_depth(&document), depth);
    for format in [FrontmatterFormat::Toml, FrontmatterFormat::Yaml] {
        let options = WriteOptions {
            frontmatter: format,
        };
        let written = write(&document, &options).unwrap();
        assert_eq!(frontmatter_depth(&parse(&written).document), depth);
    }
}

#[test]
fn a_copy_counts_toward_the_value_cap_from_where_its_alias_stands() {
    on_two_megabytes(|| {
        // The anchor counts as a level of its own.
        let lists = VALUE_CAP - 1;
        let deep = format!("{}1{}", "[".repeat(lists), "]".repeat(lists));
        let source = format!("---\na: &d {deep}\nb: [*d]\nc: [[*d]]\ne: 1\n---\n");
        let read = valid(&source).frontmatter.unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), ["a", "b", "e"]);
        reads_and_writes_at_depth(&source, Some(VALUE_CAP));
    });
}

#[test]
fn frontmatter_nested_one_level_past_the_value_cap_drops_the_innermost_pair_that_holds_it() {
    on_two_megabytes(|| {
        // A pair whose value holds a key keeps every level but the dropped one;
        // the frontmatter of any other pair keeps no entry.
        let kept = [None, Some(VALUE_CAP), None, None, None, Some(VALUE_CAP)];
        for (source, depth) in nested_frontmatter(VALUE_CAP + 1).iter().zip(kept) {
            reads_and_writes_at_depth(source, depth);
        }
    });
}

#[test]
fn a_second_tag_on_a_value_or_a_key_is_refused_where_it_starts() {
    on_two_megabytes(|| {
        for source in [
            "---\na: !!str !!str x\n---\n",
            "---\na: !point !!str x\n---\n",
            "---\n!point !!str a: 1\n---\n",
            "---\n!!str !!str a: 1\n---\n",
            "---\na: [!!str !!str x]\n---\n",
            "---\na: {!!str !!str b: 1}\n---\n",
            "---\na: !!str\n  !!str x\n---\n",
        ] {
            let error = broken(source).error;
            assert_eq!(error.message, "a value has at most one tag");
            let at = source.rfind("!!").unwrap();
            assert_eq!(error.span, at..at + 1);
        }
    });
}

/// The frontmatter of each shape of nesting whose levels each hold a tag on
/// a collection or an explicit key, `levels` deep. Each level counts twice
/// toward the value cap.
fn tagged_frontmatter(levels: usize) -> [String; 3] {
    let mut tags = String::new();
    let mut explicit = String::new();
    for level in 0..levels {
        let indent = "  ".repeat(level);
        tags.push_str(&indent);
        tags.push_str("a: !!map\n");
        explicit.push_str(&indent);
        explicit.push_str("? a\n");
        explicit.push_str(&indent);
        explicit.push_str(":\n");
    }
    let innermost = "  ".repeat(levels);
    let lists = format!("{}1{}", "!!seq [".repeat(levels), "]".repeat(levels));
    [
        format!("---\n{tags}{innermost}b: 1\n---\n"),
        format!("---\na: {lists}\n---\n"),
        format!("---\n{explicit}{innermost}b: 1\n---\n"),
    ]
}

#[test]
fn tags_and_explicit_keys_nested_to_the_value_cap_go_through_every_operation() {
    on_two_megabytes(|| {
        let levels = VALUE_CAP / 2;
        for source in tagged_frontmatter(levels) {
            let document = parse(&source).document;
            assert_eq!(frontmatter_depth(&document), Some(levels));
            for format in [FrontmatterFormat::Toml, FrontmatterFormat::Yaml] {
                let options = WriteOptions {
                    frontmatter: format,
                };
                let written = write(&document, &options).unwrap();
                let depth = frontmatter_depth(&parse(&written).document);
                assert_eq!(depth, Some(levels));
            }
            let copy = document.clone();
            assert_eq!(copy, document);
            let printed = format!("{document:?}");
            assert!(printed.starts_with("Valid(Document {"));
            drop(copy);
            let json = to_json(&document).unwrap();
            let back: RawDocument = from_json(&json).unwrap();
            assert_eq!(to_json(&back).unwrap(), json);
            drop(back);
            drop(document);
        }
    });
}

#[test]
fn tags_and_explicit_keys_nested_one_level_past_the_value_cap_drop_the_innermost_pair() {
    on_two_megabytes(|| {
        let levels = VALUE_CAP / 2;
        let kept = [Some(levels), None, Some(levels)];
        for (source, depth) in tagged_frontmatter(levels + 1).iter().zip(kept) {
            reads_and_writes_at_depth(source, depth);
        }
    });
}
