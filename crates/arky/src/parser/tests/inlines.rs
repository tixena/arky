use core::ops::Range;
use std::thread;

use super::{check, check_spans, on_two_megabytes};
use crate::parser::ast::{
    Block, Continues, Inline, Kind, Link, LinkForm, LinkInline, RawDocument, Text,
};
use crate::parser::extension::Markdown;
use crate::parser::inline::INLINE_CAP;
use crate::parser::{located, parse};
use crate::writer::{WriteOptions, write};

#[test]
fn a_wiki_link_has_a_target_and_may_have_an_alias() {
    check(
        "See [[grammar]], [[notes/parser|the parser notes]] and [[spec#Lists]].\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "See "
          Link form=Wiki target="grammar"
          Text ", "
          Link form=Wiki target="notes/parser"
            Text "the parser notes"
          Text " and "
          Link form=Wiki target="spec#Lists"
          Text "."
"#,
    );
}

#[test]
fn a_link_holds_its_text_as_children_and_its_destination() {
    check(
        "Read [the spec](docs/spec.md) or [`parse`](https://example.com/parse?x=1).\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Read "
          Link form=Markdown target="docs/spec.md"
            Text "the spec"
          Text " or "
          Link form=Markdown target="https://example.com/parse?x=1"
            Code "parse"
          Text "."
"#,
    );
}

#[test]
fn a_code_span_keeps_what_is_inside_it_as_written() {
    check(
        "Use `[[not a link]]` and ``a ` tick`` here.\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Use "
          Code "[[not a link]]"
          Text " and "
          Code "a ` tick"
          Text " here."
"#,
    );
}

#[test]
fn a_backslash_before_punctuation_makes_it_plain_text() {
    check(
        r"\[\[not a link\]\] and \* star, a path C:\temp
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[[not a link]] and * star, a path C:\\temp"
"#,
    );
}

#[test]
fn brackets_that_do_not_form_a_link_stay_text() {
    check(
        "[text] (gap), [text](has space), [[]], [[a]b]], [unclosed, an ![image](pic.png)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[text] (gap), [text](has space), [[]], [[a]b]], [unclosed, an "
          Image form=Markdown target="pic.png" alt="image"
"#,
    );
}

#[test]
fn each_line_of_a_paragraph_holds_the_inline_nodes_written_on_it() {
    check(
        "\
Start [[a]]
[b](c) end
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Start "
          Link form=Wiki target="a"
        Line
          Link form=Markdown target="c"
            Text "b"
          Text " end"
"#,
    );
}

#[test]
fn backticks_with_no_matching_run_stay_text() {
    check(
        "A lone ` tick.\n\n``two` ticks\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "A lone ` tick."
      Paragraph
        Line
          Text "``two` ticks"
"#,
    );
}

#[test]
fn text_outside_ascii_is_kept_and_spans_stay_on_character_boundaries() {
    check(
        "+++\ntitle = \"caf\u{e9}\"\n+++\ncaf\u{e9} [[na\u{ef}ve|\u{f1}]] `\u{2014}`\n",
        "\
Document
  Frontmatter format=Toml
    Mapping
      Pair key=title
        Scalar String \"caf\u{e9}\"
  MarkdownBlock
    Section
      Paragraph
        Line
          Text \"caf\u{e9} \"
          Link form=Wiki target=\"na\u{ef}ve\"
            Text \"\u{f1}\"
          Text \" \"
          Code \"\u{2014}\"
",
    );
}

#[test]
fn a_link_may_cross_a_line_end_and_link_text_may_hold_balanced_brackets() {
    check(
        "\
[text
more](dest) and [a [b] c](d)
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="dest" continues=next
            Text "text"
        Line
          Link form=Markdown target="dest" continues=previous
            Text "more"
          Text " and "
          Link form=Markdown target="d"
            Text "a [b] c"
"#,
    );
}

#[test]
fn an_image_is_an_embed_in_either_form_and_may_sit_inside_a_link() {
    check(
        "Hi! ![logo](logo.svg) ![[tree.png]] ![[note#Heading|the note]] [![build](badge.svg)](https://ci) ![not an image\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Hi! "
          Image form=Markdown target="logo.svg" alt="logo"
          Text " "
          Image form=Wiki target="tree.png"
          Text " "
          Image form=Wiki target="note#Heading" alt="the note"
          Text " "
          Link form=Markdown target="https://ci"
            Image form=Markdown target="badge.svg" alt="build"
          Text " ![not an image"
"#,
    );
}

#[test]
fn emphasis_and_strong_are_read_from_their_delimiters() {
    check(
        "*emphasis* and **strong** are read.\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis
            Text "emphasis"
          Text " and "
          Strong
            Text "strong"
          Text " are read."
"#,
    );
}

#[test]
fn reference_links_autolinks_and_inline_html_are_read() {
    check(
        "\
[label][ref] and <https://example.com> and <b>html</b>

[ref]: https://example.com
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="https://example.com"
            Text "label"
          Text " and "
          Link form=Markdown target="https://example.com"
            Text "https://example.com"
          Text " and "
          Html "<b>"
          Text "html"
          Html "</b>"
"#,
    );
}

#[test]
fn one_delimiter_is_emphasis_and_two_are_strong_with_either_character() {
    check(
        "*a* _b_ **c** __d__\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis
            Text "a"
          Text " "
          Emphasis
            Text "b"
          Text " "
          Strong
            Text "c"
          Text " "
          Strong
            Text "d"
"#,
    );
}

#[test]
fn three_delimiters_are_emphasis_around_strong() {
    check(
        "***c***\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis
            Strong
              Text "c"
"#,
    );
}

#[test]
fn an_underscore_inside_a_word_opens_no_emphasis() {
    check(
        "snake_case_word and _foo_bar_\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "snake_case_word and "
          Emphasis
            Text "foo_bar"
"#,
    );
}

#[test]
fn delimiter_runs_pair_by_the_rules_of_commonmark() {
    check(
        "\
**foo*

*foo**

*foo**bar**baz*

*(*foo*)*

foo-_(bar)_

***a** b*

**a*b*c**

a * not emphasis *
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "*"
          Emphasis
            Text "foo"
      Paragraph
        Line
          Emphasis
            Text "foo"
          Text "*"
      Paragraph
        Line
          Emphasis
            Text "foo"
            Strong
              Text "bar"
            Text "baz"
      Paragraph
        Line
          Emphasis
            Text "("
            Emphasis
              Text "foo"
            Text ")"
      Paragraph
        Line
          Text "foo-"
          Emphasis
            Text "(bar)"
      Paragraph
        Line
          Emphasis
            Strong
              Text "a"
            Text " b"
      Paragraph
        Line
          Strong
            Text "a"
            Emphasis
              Text "b"
            Text "c"
      Paragraph
        Line
          Text "a * not emphasis *"
"#,
    );
}

#[test]
fn an_escaped_delimiter_is_text() {
    check(
        r"\*not emphasis\* and *\*starred\**
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "*not emphasis* and "
          Emphasis
            Text "*starred*"
"#,
    );
}

#[test]
fn inline_html_is_an_html_node_as_written() {
    check(
        "a <span class=\"x\">b</span> c <br> d <!-- note --> <?php x ?> <!DOCTYPE html> <![CDATA[ y ]]> <a/>\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a "
          Html "<span class=\"x\">"
          Text "b"
          Html "</span>"
          Text " c "
          Html "<br>"
          Text " d "
          Html "<!-- note -->"
          Text " "
          Html "<?php x ?>"
          Text " "
          Html "<!DOCTYPE html>"
          Text " "
          Html "<![CDATA[ y ]]>"
          Text " "
          Html "<a/>"
"#,
    );
}

#[test]
fn a_less_than_sign_that_starts_no_tag_is_text() {
    check(
        "1 < 2, <a b=>, <1a>, <not closed\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "1 < 2, <a b=>, <1a>, <not closed"
"#,
    );
}

#[test]
fn an_autolink_is_a_link_to_what_it_shows() {
    check(
        "<https://example.com> and <ada@example.com>\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="https://example.com"
            Text "https://example.com"
          Text " and "
          Link form=Markdown target="mailto:ada@example.com"
            Text "ada@example.com"
"#,
    );
}

#[test]
fn a_backslash_or_two_spaces_before_another_line_end_their_line_with_a_line_break() {
    check(
        "first\\\nsecond  \nthird\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "first"
          LineBreak
        Line
          Text "second"
          LineBreak
        Line
          Text "third"
"#,
    );
}

#[test]
fn the_last_line_of_a_paragraph_ends_with_no_line_break() {
    check(
        "a\\\n\nb  \n\nc \\\nd\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a\\"
      Paragraph
        Line
          Text "b"
      Paragraph
        Line
          Text "c "
          LineBreak
        Line
          Text "d"
"#,
    );
}

#[test]
fn one_space_or_an_escaped_backslash_at_the_end_of_a_line_is_no_line_break() {
    check(
        "a \nb\\ \nc\\\\\nd\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "a"
        Line
          Text "b\\"
        Line
          Text "c\\"
        Line
          Text "d"
"#,
    );
}

#[test]
fn a_line_break_inside_an_underlined_heading_stays_in_it() {
    check(
        "a\\\nb\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "a"
        LineBreak
        Text "b"
"#,
    );
}

#[test]
fn a_reference_link_takes_its_target_from_the_definition_of_its_label() {
    check(
        "\
[x][1] [1][] [1] and [X][Guide] [guide][] [GUIDE]

[1]: u
[guide]: /g
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="u"
            Text "x"
          Text " "
          Link form=Markdown target="u"
            Text "1"
          Text " "
          Link form=Markdown target="u"
            Text "1"
          Text " and "
          Link form=Markdown target="/g"
            Text "X"
          Text " "
          Link form=Markdown target="/g"
            Text "guide"
          Text " "
          Link form=Markdown target="/g"
            Text "GUIDE"
"#,
    );
}

#[test]
fn a_label_with_no_definition_stays_text() {
    check(
        "[the guide][nope], [nope][] and [nope]\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[the guide][nope], [nope][] and [nope]"
"#,
    );
}

#[test]
fn brackets_followed_by_a_label_are_no_shortcut_and_an_inline_link_comes_first() {
    check(
        "\
[foo][bar][baz] and [foo](not a link)

[baz]: /url1
[foo]: /url2
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[foo]"
          Link form=Markdown target="/url1"
            Text "bar"
          Text " and "
          Link form=Markdown target="/url2"
            Text "foo"
          Text "(not a link)"
"#,
    );
}

#[test]
fn inline_nodes_nest_inside_links_emphasis_and_strong() {
    check(
        "[*the* guide](x) and **[a](x)** and *[b*](x)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="x"
            Emphasis
              Text "the"
            Text " guide"
          Text " and "
          Strong
            Link form=Markdown target="x"
              Text "a"
          Text " and *"
          Link form=Markdown target="x"
            Text "b*"
"#,
    );
}

#[test]
fn a_code_span_inside_link_text_may_hold_a_bracket() {
    check(
        "[a `]` b](x)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="x"
            Text "a "
            Code "]"
            Text " b"
"#,
    );
}

#[test]
fn a_paragraph_holds_strong_links_line_breaks_and_html_together() {
    check(
        "Read **[the guide][1]**,\\\nthen <span>stop</span>.\n\n[1]: https://example.com/guide\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Read "
          Strong
            Link form=Markdown target="https://example.com/guide"
              Text "the guide"
          Text ","
          LineBreak
        Line
          Text "then "
          Html "<span>"
          Text "stop"
          Html "</span>"
          Text "."
"#,
    );
}

#[test]
fn labels_match_by_unicode_case_folding_and_collapsed_whitespace() {
    check(
        "[\u{1E9E}] and [Foo Bar]\n\n[SS]: /url\n[foo   bar]: /u\n",
        "\
Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target=\"/url\"
            Text \"\u{1E9E}\"
          Text \" and \"
          Link form=Markdown target=\"/u\"
            Text \"Foo Bar\"
",
    );
}

#[test]
fn a_destination_is_held_with_its_escapes_and_references_resolved() {
    check(
        "\
[a](/u\\_x) [b] [c](/u&amp;v) [d](</my url>) [e](/&#35;&#x41;&#0;&bogus;)

[b]: /u\\_x
",
        "\
Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target=\"/u_x\"
            Text \"a\"
          Text \" \"
          Link form=Markdown target=\"/u_x\"
            Text \"b\"
          Text \" \"
          Link form=Markdown target=\"/u&v\"
            Text \"c\"
          Text \" \"
          Link form=Markdown target=\"/my url\"
            Text \"d\"
          Text \" \"
          Link form=Markdown target=\"/#A\u{FFFD}&bogus;\"
            Text \"e\"
",
    );
}

#[test]
fn a_link_or_an_image_keeps_its_title_in_any_of_its_forms() {
    check(
        "\
[a](/u \"T\") [b](/u 'T') [c](/u (T)) [d] [e](/u \"T&amp;\\\"q\") [f](/u) ![g](/i \"T\")

[d]: /u \"T\"
",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="/u" title="T"
            Text "a"
          Text " "
          Link form=Markdown target="/u" title="T"
            Text "b"
          Text " "
          Link form=Markdown target="/u" title="T"
            Text "c"
          Text " "
          Link form=Markdown target="/u" title="T"
            Text "d"
          Text " "
          Link form=Markdown target="/u" title="T&\"q"
            Text "e"
          Text " "
          Link form=Markdown target="/u"
            Text "f"
          Text " "
          Image form=Markdown target="/i" title="T" alt="g"
"#,
    );
}

#[test]
fn a_reference_link_with_a_title_and_an_escaped_destination() {
    check(
        "[\u{1E9E}][] and [b](/u\\_x \"T\")\n\n[SS]: /url\n",
        "\
Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target=\"/url\"
            Text \"\u{1E9E}\"
          Text \" and \"
          Link form=Markdown target=\"/u_x\" title=\"T\"
            Text \"b\"
",
    );
}

#[test]
fn flanking_counts_unicode_punctuation_and_whitespace() {
    check(
        "a*\u{201C}foo\u{201D}* and a*\"foo\"* and *\u{A0}a* and *\u{A3}x\u{A3}*\n",
        "\
Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text \"a*\u{201C}foo\u{201D}* and a*\\\"foo\\\"* and *\\u{a0}a* and \"
          Emphasis
            Text \"\u{A3}x\u{A3}\"
",
    );
}

#[test]
fn emphasis_across_a_line_end_is_cut_into_a_piece_on_each_line() {
    check(
        "Some *emphasis that\ngoes on* here.\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Some "
          Emphasis continues=next
            Text "emphasis that"
        Line
          Emphasis continues=previous
            Text "goes on"
          Text " here."
"#,
    );
}

#[test]
fn an_opening_with_no_closing_in_its_paragraph_stays_text() {
    check(
        "Some *emphasis that\ngoes on here.\n\nThis is not emphasis\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "Some *emphasis that"
        Line
          Text "goes on here."
      Paragraph
        Line
          Text "This is not emphasis"
"#,
    );
}

#[test]
fn a_middle_piece_goes_on_both_ways() {
    check(
        "*a\nb\nc*\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis continues=next
            Text "a"
        Line
          Emphasis continues=both
            Text "b"
        Line
          Emphasis continues=previous
            Text "c"
"#,
    );
}

#[test]
fn nested_nodes_across_a_line_end_are_each_cut() {
    check(
        "**a *b\nc* d**\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Strong continues=next
            Text "a "
            Emphasis continues=next
              Text "b"
        Line
          Strong continues=previous
            Emphasis continues=previous
              Text "c"
            Text " d"
"#,
    );
}

#[test]
fn delimiter_runs_pair_across_a_line_end() {
    check(
        "**foo*\n*foo**\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis continues=next
            Emphasis
              Text "foo"
        Line
          Emphasis continues=previous
            Emphasis
              Text "foo"
"#,
    );
}

#[test]
fn a_code_span_across_a_line_end_holds_the_part_on_each_line() {
    check(
        "`a\nb`\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Code "a" continues=next
        Line
          Code "b" continues=previous
"#,
    );
}

#[test]
fn each_piece_of_a_link_holds_its_target() {
    check(
        "[a\nb](u)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="u" continues=next
            Text "a"
        Line
          Link form=Markdown target="u" continues=previous
            Text "b"
"#,
    );
}

#[test]
fn a_piece_of_a_link_may_hold_no_text() {
    check(
        "[a](u\n\"t\")\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="u" title="t" continues=next
            Text "a"
        Line
          Link form=Markdown target="u" title="t" continues=previous
"#,
    );
}

#[test]
fn each_piece_of_an_image_holds_the_alt_text_on_its_line() {
    check(
        "![a\nb](u)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="u" alt="a" continues=next
        Line
          Image form=Markdown target="u" alt="b" continues=previous
"#,
    );
}

#[test]
fn inline_html_across_a_line_end_holds_what_is_written_on_each_line() {
    check(
        "<span\nclass=\"x\">a</span>\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Html "<span" continues=next
        Line
          Html "class=\"x\">" continues=previous
          Text "a"
          Html "</span>"
"#,
    );
}

#[test]
fn a_hard_line_break_ends_the_first_piece() {
    check(
        "*a\\\nb*\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Emphasis continues=next
            Text "a"
            LineBreak
        Line
          Emphasis continues=previous
            Text "b"
"#,
    );
}

#[test]
fn an_underlined_heading_holds_whole_nodes() {
    check(
        "x *a\nb*\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "x "
        Emphasis
          Text "a b"
"#,
    );
}

#[test]
fn an_underlined_heading_joins_the_pieces_of_each_node() {
    check(
        "`a\nb` [c\nd](u) ![e\nf](u) <span\nclass=\"x\">\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Code "a b"
        Text " "
        Link form=Markdown target="u"
          Text "c d"
        Text " "
        Image form=Markdown target="u" alt="e f"
        Text " "
        Html "<span\nclass=\"x\">"
"#,
    );
}

#[test]
fn a_code_span_does_not_cross_a_blank_line() {
    check(
        "`a\n\nb`\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "`a"
      Paragraph
        Line
          Text "b`"
"#,
    );
}

#[test]
fn a_reference_image_takes_its_target_and_title_from_the_definition_of_its_label() {
    check(
        "![a][1]\n\n[1]: /u\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="/u" alt="a"
"#,
    );
    check(
        "![a][]\n\n[a]: /u \"T\"\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="/u" title="T" alt="a"
"#,
    );
    check(
        "![a]\n\n[a]: /u\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="/u" alt="a"
"#,
    );
}

#[test]
fn a_reference_image_whose_label_has_no_definition_stays_text() {
    check(
        "![a][9]\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "![a][9]"
"#,
    );
}

#[test]
fn entity_and_numeric_character_references_in_text_are_resolved() {
    let line = "a &amp; b &copy; &#35; &#x22; &foo; &#0;";
    let whole = "a &amp; b &copy; &#35; &#x22; &foo; &#0;\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", line),
            ("Paragraph", line),
            ("Line", line),
            ("Text \"a & b \u{a9} # \\\" &foo; \u{fffd}\"", line),
        ],
    );
}

#[test]
fn a_reference_stays_as_written_in_a_code_span_an_autolink_and_inline_html() {
    check(
        "`&amp;` <https://x?a&amp;b> <b title=\"&amp;\">\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Code "&amp;"
          Text " "
          Link form=Markdown target="https://x?a&amp;b"
            Text "https://x?a&amp;b"
          Text " "
          Html "<b title=\"&amp;\">"
"#,
    );
}

#[test]
fn a_reference_in_a_heading_is_resolved() {
    check(
        "# A &amp; B\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "A & B"
"#,
    );
}

#[test]
fn link_text_and_image_text_may_hold_balanced_brackets() {
    check(
        "[a [b] c](d)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="d"
            Text "a [b] c"
"#,
    );
    check(
        "![a [b] c](d)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="d" alt="a [b] c"
"#,
    );
    check(
        "[a [b] c][x]\n\n[x]: /u\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="/u"
            Text "a [b] c"
"#,
    );
}

#[test]
fn a_link_inside_link_text_leaves_the_outer_brackets_text() {
    check(
        "[a [b](c) d](e)\n\n[a [b c](d)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[a "
          Link form=Markdown target="c"
            Text "b"
          Text " d](e)"
      Paragraph
        Line
          Text "[a "
          Link form=Markdown target="d"
            Text "b c"
"#,
    );
}

#[test]
fn a_code_span_loses_one_space_at_each_end_when_both_ends_are_spaces() {
    let whole = "` a ` and `  b  ` and ` ` and `` `a` ``\n";
    let line = "` a ` and `  b  ` and ` ` and `` `a` ``";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", line),
            ("Paragraph", line),
            ("Line", line),
            (r#"Code "a""#, "` a `"),
            (r#"Text " and ""#, " and "),
            (r#"Code " b ""#, "`  b  `"),
            (r#"Text " and ""#, " and "),
            (r#"Code " ""#, "` `"),
            (r#"Text " and ""#, " and "),
            (r#"Code "`a`""#, "`` `a` ``"),
        ],
    );
}

#[test]
fn a_code_span_across_a_line_end_keeps_the_spaces_before_it() {
    let whole = "`foo   bar \nbaz`\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "`foo   bar \nbaz`"),
            ("Paragraph", "`foo   bar \nbaz`"),
            ("Line", "`foo   bar "),
            (r#"Code "foo   bar " continues=next"#, "`foo   bar "),
            ("Line", "baz`"),
            (r#"Code "baz" continues=previous"#, "baz`"),
        ],
    );
}

#[test]
fn two_spaces_before_a_line_end_inside_a_code_span_are_code_and_no_line_break() {
    let whole = "`a  \nb`\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "`a  \nb`"),
            ("Paragraph", "`a  \nb`"),
            ("Line", "`a  "),
            (r#"Code "a  " continues=next"#, "`a  "),
            ("Line", "b`"),
            (r#"Code "b" continues=previous"#, "b`"),
        ],
    );
}

#[test]
fn an_underlined_heading_joins_a_code_span_with_the_spaces_before_its_line_end() {
    check(
        "x `foo   bar \nbaz`\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "x "
        Code "foo   bar  baz"
"#,
    );
}

#[test]
fn a_paragraph_reads_reference_images_references_brackets_and_code_span_ends() {
    check(
        "![logo][l] &amp; [a [b] c](d) ` x `\n\n[l]: /logo.svg \"Logo\"\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Image form=Markdown target="/logo.svg" title="Logo" alt="logo"
          Text " & "
          Link form=Markdown target="d"
            Text "a [b] c"
          Text " "
          Code "x"
"#,
    );
}

#[test]
fn inline_html_and_a_link_title_keep_the_spaces_before_a_line_end() {
    check(
        "x <span  \nclass=\"x\">\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "x "
          Html "<span  " continues=next
        Line
          Html "class=\"x\">" continues=previous
"#,
    );
    check(
        "[a](u \"t  \nx\")\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="u" title="t  \nx" continues=next
            Text "a"
        Line
          Link form=Markdown target="u" title="t  \nx" continues=previous
"#,
    );
    check(
        "a \nb\n",
        r#"Document
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
fn a_line_ending_at_an_end_of_a_code_span_counts_as_a_space_and_stays_between_its_pieces() {
    check(
        "` a\n`\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Code "a" continues=next
        Line
          Code "" continues=previous
"#,
    );
    check(
        "x ` a\n`\n===\n",
        r#"Document
  MarkdownBlock
    Section
      Heading
        Text "x "
        Code "a"
"#,
    );
}

#[test]
fn a_link_inside_an_image_inside_link_text_leaves_the_outer_brackets_text() {
    check(
        "[a ![[b](c)](d) e](f)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[a "
          Image form=Markdown target="d" alt="[b](c)"
          Text " e](f)"
"#,
    );
}

#[test]
fn an_autolink_in_link_text_stays_a_link_and_the_brackets_around_it_stay_text() {
    check(
        "[<https://a.example>](https://b.example)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "["
          Link form=Markdown target="https://a.example"
            Text "https://a.example"
          Text "](https://b.example)"
"#,
    );
}

#[test]
fn an_autolink_in_an_emphasis_in_link_text_stays_a_link_and_the_brackets_stay_text() {
    check(
        "[*<https://a.example>*](https://b.example)\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "["
          Emphasis
            Link form=Markdown target="https://a.example"
              Text "https://a.example"
          Text "](https://b.example)"
"#,
    );
}

#[test]
fn a_hand_built_link_inside_a_link_moves_into_the_model_as_its_text() {
    let located_text = |value: &str, span: Range<usize>| {
        located::Inline::Text(located::Text {
            kind: Kind(None),
            span,
            value: value.to_owned(),
        })
    };
    let located_link = |children, target: &str, span| {
        located::Inline::Link(located::Link {
            children,
            continues: Continues::Neither,
            form: LinkForm::Markdown,
            kind: Kind(None),
            span,
            target: target.to_owned(),
            title: None,
        })
    };
    let inner = located_link(vec![located_text("b", 4..5)], "c", 3..9);
    let line = located::Line::<Markdown> {
        children: vec![located_link(
            vec![located_text("a ", 1..3), inner, located_text(" d", 9..11)],
            "e",
            0..15,
        )],
        kind: Kind(None),
        span: 0..15,
    };
    let model_text = |value: &str| {
        LinkInline::Text(Text {
            kind: Kind(None),
            value: value.to_owned(),
        })
    };
    let expected = Inline::Link(Link {
        children: vec![model_text("a "), model_text("b"), model_text(" d")],
        continues: Continues::Neither,
        form: LinkForm::Markdown,
        kind: Kind(None),
        target: "e".to_owned(),
        title: None,
    });
    assert_eq!(line.into_model().children, [expected]);
}

#[test]
fn ten_thousand_brackets_around_a_word_are_text_around_a_wiki_link() {
    let source = format!("{}x{}\n", "[".repeat(10_000), "]".repeat(10_000));
    let expected = format!(
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "{}"
          Link form=Wiki target="x"
          Text "{}"
"#,
        "[".repeat(9_998),
        "]".repeat(9_998)
    );
    check(&source, &expected);
}

#[test]
fn of_links_nested_two_thousand_deep_only_the_innermost_is_a_link() {
    let source = format!("{}a{}\n", "[".repeat(2_000), "](u)".repeat(2_000));
    let expected = format!(
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "{}"
          Link form=Markdown target="u"
            Text "a"
          Text "{}"
"#,
        "[".repeat(1_999),
        "](u)".repeat(1_999)
    );
    check(&source, &expected);
}

#[test]
fn a_run_of_angle_brackets_is_text_and_an_autolink_search_stops_at_a_bracket() {
    check(
        "<<<<<x\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "<<<<<x"
"#,
    );
    check(
        "<a<http://b>\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "<a"
          Link form=Markdown target="http://b"
            Text "http://b"
"#,
    );
}

#[test]
fn empty_brackets_before_unclosed_parentheses_are_text() {
    check(
        "[]([]([](\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "[]([]([]("
"#,
    );
}

#[test]
fn backtick_runs_that_close_nothing_are_text() {
    check(
        "`a``a```a````a`````a\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "`a``a```a````a`````a"
"#,
    );
}

#[test]
fn a_destination_holds_parentheses_nested_at_most_thirty_two_deep() {
    check(
        "[a](((((b)))))\n",
        r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="((((b))))"
            Text "a"
"#,
    );
    let deepest = format!("{}b{}", "(".repeat(32), ")".repeat(32));
    check(
        &format!("[a]({deepest})\n"),
        &format!(
            r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Link form=Markdown target="{deepest}"
            Text "a"
"#
        ),
    );
    let deeper = format!("[a]({}b{})", "(".repeat(33), ")".repeat(33));
    check(
        &format!("{deeper}\n"),
        &format!(
            r#"Document
  MarkdownBlock
    Section
      Paragraph
        Line
          Text "{deeper}"
"#
        ),
    );
}

#[test]
fn emphasis_nested_two_thousand_deep_reads_writes_and_drops_on_a_two_megabyte_stack() {
    let reader = thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let source = format!("{}a{}\n", "*".repeat(2_000), "*".repeat(2_000));
            let document = parse(&source).document;
            let mut nodes = only_line(&document);
            let mut depth = 0_usize;
            while let [Inline::Strong(strong)] = nodes {
                depth += 1;
                nodes = &strong.children;
            }
            assert_eq!(depth, 1_000);
            assert!(matches!(nodes, [Inline::Text(text)] if text.value == "a"));
            let written = write(&document, &WriteOptions::default()).unwrap();
            assert_eq!(written, source);
            drop(document);
        })
        .unwrap();
    reader.join().unwrap();
}

/// The nodes of the one line of the paragraph that `document` holds.
fn only_line(document: &RawDocument) -> &[Inline] {
    let RawDocument::Valid(valid) = document else {
        return &[];
    };
    match valid.body.sections[0].blocks.as_slice() {
        [Block::Paragraph(paragraph)] => paragraph.lines[0].children.as_slice(),
        _ => &[],
    }
}

/// How deep strong emphasis nests from `nodes` down, and what the innermost
/// one holds.
fn strong_depth(nodes: &[Inline]) -> (usize, &[Inline]) {
    let mut inner = nodes;
    let mut depth = 0_usize;
    while let [Inline::Strong(strong)] = inner {
        depth += 1;
        inner = &strong.children;
    }
    (depth, inner)
}

/// How deep strong emphasis nests from `nodes`, a link's text, down.
fn link_strong_depth(nodes: &[LinkInline]) -> usize {
    let mut inner = nodes;
    let mut depth = 0_usize;
    while let [LinkInline::Strong(strong)] = inner {
        depth += 1;
        inner = &strong.children;
    }
    depth
}

/// Checks that the one line of `document` is the text `stars`, strong
/// emphasis nested `INLINE_CAP` deep around `a`, then `stars` again.
fn assert_inline_capped(document: &RawDocument, stars: &str) {
    let nodes = only_line(document);
    assert!(
        matches!(nodes, [Inline::Text(before), Inline::Strong(_), Inline::Text(after)]
            if before.value == stars && after.value == stars)
    );
    let (depth, innermost) = strong_depth(&nodes[1..2]);
    assert_eq!(depth, INLINE_CAP);
    assert!(matches!(innermost, [Inline::Text(text)] if text.value == "a"));
}

#[test]
fn emphasis_nests_the_inline_cap_deep_and_the_stars_past_it_stay_text() {
    on_two_megabytes(|| {
        let marks = "*".repeat(10_000);
        let document = parse(&format!("{marks}a{marks}\n")).document;
        let stars = "*".repeat(10_000 - 2 * INLINE_CAP);
        assert_inline_capped(&document, &stars);
        let written = write(&document, &WriteOptions::default()).unwrap();
        let escaped = "\\*".repeat(stars.len());
        let paired = "*".repeat(2 * INLINE_CAP);
        assert_eq!(written, format!("{escaped}{paired}a{paired}{escaped}\n"));
        assert_inline_capped(&parse(&written).document, &stars);
        let copy = document.clone();
        assert_eq!(copy, document);
        let printed = format!("{document:?}");
        assert_eq!(printed.matches("Strong(").count(), INLINE_CAP);
        drop(copy);
        drop(document);
    });
}

#[test]
fn emphasis_at_the_inline_cap_reads_back_from_its_written_text_to_the_same_tree() {
    on_two_megabytes(|| {
        let marks = "*".repeat(2 * INLINE_CAP);
        let source = format!("{marks}a{marks}\n");
        let document = parse(&source).document;
        let (depth, innermost) = strong_depth(only_line(&document));
        assert_eq!(depth, INLINE_CAP);
        assert!(matches!(innermost, [Inline::Text(text)] if text.value == "a"));
        let written = write(&document, &WriteOptions::default()).unwrap();
        assert_eq!(written, source);
        assert_eq!(parse(&written).document, document);
    });
}

#[test]
fn a_link_that_would_nest_past_the_inline_cap_keeps_its_brackets_as_text() {
    on_two_megabytes(|| {
        let marks = "*".repeat(2 * INLINE_CAP);
        let document = parse(&format!("[{marks}a{marks}](u)\n")).document;
        let nodes = only_line(&document);
        assert!(
            matches!(nodes, [Inline::Text(open), Inline::Strong(_), Inline::Text(close)]
                if open.value == "[" && close.value == "](u)")
        );
        assert_eq!(strong_depth(&nodes[1..2]).0, INLINE_CAP);
        let fewer = "*".repeat(2 * (INLINE_CAP - 1));
        let linked = parse(&format!("[{fewer}a{fewer}](u)\n")).document;
        let depth = match only_line(&linked) {
            [Inline::Link(link)] if link.target == "u" => link_strong_depth(&link.children),
            _ => 0,
        };
        assert_eq!(depth, INLINE_CAP - 1);
    });
}

/// The outline of a document of one paragraph of one line, whose nodes'
/// outlines are `nodes`.
fn one_line(nodes: &str) -> String {
    format!("Document\n  MarkdownBlock\n    Section\n      Paragraph\n        Line\n{nodes}")
}

#[test]
fn a_hundred_emphasis_spans_on_one_line_are_each_emphasis() {
    let span = "          Emphasis\n            Text \"a\"\n";
    let nodes = [span; 100].join("          Text \" \"\n");
    check(&format!("{}\n", "*a* ".repeat(100)), &one_line(&nodes));
}

#[test]
fn four_hundred_code_spans_on_one_line_are_each_a_code_node() {
    let nodes = ["          Code \"a\"\n"; 400].join("          Text \" \"\n");
    check(&format!("{}\n", "`a` ".repeat(400)), &one_line(&nodes));
}

#[test]
fn a_hundred_unclosed_comments_after_text_are_text() {
    let text = format!("x {}", "<!--".repeat(100));
    check(
        &format!("{text}\n"),
        &one_line(&format!("          Text \"{text}\"\n")),
    );
}

#[test]
fn a_closed_comment_and_a_processing_instruction_are_inline_html() {
    check(
        "x <!-- a --> b <? c ?> d\n",
        &one_line(
            r#"          Text "x "
          Html "<!-- a -->"
          Text " b "
          Html "<? c ?>"
          Text " d"
"#,
        ),
    );
}

#[test]
fn a_hundred_closers_after_a_hundred_code_spans_are_text() {
    let codes = ["          Code \"a\"\n"; 100].join("          Text \" \"\n");
    let text = format!(" {}", "a* ".repeat(100).trim_end());
    check(
        &format!("{}{}\n", "`a` ".repeat(100), "a* ".repeat(100)),
        &one_line(&format!("{codes}          Text \"{text}\"\n")),
    );
}

#[test]
fn a_hundred_closers_after_a_hundred_openers_nest_emphasis_a_hundred_deep() {
    let indent = |level: usize| "  ".repeat(5 + level);
    let opened = (0..99)
        .map(|level| {
            let pad = indent(level);
            format!("{pad}Emphasis\n{pad}  Text \"a \"\n")
        })
        .collect::<Vec<_>>()
        .concat();
    let pad = indent(99);
    let innermost = format!("{pad}Emphasis\n{pad}  Text \"a b\"\n");
    let closed = (0..99)
        .rev()
        .map(|level| format!("{}  Text \" b\"\n", indent(level)))
        .collect::<Vec<_>>()
        .concat();
    check(
        &format!("{}{}\n", "*a ".repeat(100), "b* ".repeat(100)),
        &one_line(&format!("{opened}{innermost}{closed}")),
    );
}
