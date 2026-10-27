use super::check_spans;
use crate::parser::ast::Kind;
use crate::parser::extension::Markdown;
use crate::parser::located::Heading;
use crate::parser::{ParseOptions, parse_located};

#[test]
fn a_line_spans_its_own_text_and_a_paragraph_spans_its_lines() {
    let whole = "- a\n  b\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "- a\n  b"),
            ("List", "- a\n  b"),
            ("Item", "- a\n  b"),
            ("MarkdownBlock", "a\n  b"),
            ("Section", "a\n  b"),
            ("Paragraph", "a\n  b"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            ("Line", "b"),
            (r#"Text "b""#, "b"),
        ],
    );
}

#[test]
fn a_section_runs_from_its_heading_to_the_end_of_what_it_holds() {
    let whole = "# A\n\ntext\n\n## B\n\n# C\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "# A\n\ntext\n\n## B"),
            ("Heading", "# A"),
            (r#"Text "A""#, "A"),
            ("Paragraph", "text"),
            ("Line", "text"),
            (r#"Text "text""#, "text"),
            ("Section", "## B"),
            ("Heading", "## B"),
            (r#"Text "B""#, "B"),
            ("Section", "# C"),
            ("Heading", "# C"),
            (r#"Text "C""#, "C"),
        ],
    );
}

#[test]
fn an_empty_heading_has_an_empty_span_at_the_start_of_its_section() {
    let source = "intro\n\n# A\n### C\n##\n";
    let sections = parse_located::<Markdown>(source, &ParseOptions::default())
        .body
        .sections;
    assert_eq!(sections.len(), 2);
    let subsections = &sections[1].subsections;
    assert_eq!(subsections.len(), 2);
    for (section, covered, written) in [
        (&sections[0], "intro", false),
        (&subsections[0], "### C", false),
        (&subsections[1], "##", true),
    ] {
        assert_eq!(&source[section.span.clone()], covered);
        let start = section.span.start;
        let empty = Heading {
            children: Vec::new(),
            kind: Kind(None),
            span: start..start,
        };
        assert_eq!(section.heading, written.then_some(empty));
    }
}

#[test]
fn a_code_block_spans_from_its_opening_fence_to_its_closing_fence() {
    let whole = "- x\n  ```sh\n  ls\n  ```\n";
    let item = "- x\n  ```sh\n  ls\n  ```";
    let content = "x\n  ```sh\n  ls\n  ```";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", item),
            ("List", item),
            ("Item", item),
            ("MarkdownBlock", content),
            ("Section", content),
            ("Paragraph", "x"),
            ("Line", "x"),
            (r#"Text "x""#, "x"),
            (
                r#"CodeBlock language="sh" value="ls""#,
                "```sh\n  ls\n  ```",
            ),
        ],
    );
}

#[test]
fn inline_nodes_span_their_delimiters() {
    let whole = "a [b](c) `d` [[e|f]]\n";
    let line = "a [b](c) `d` [[e|f]]";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", line),
            ("Paragraph", line),
            ("Line", line),
            (r#"Text "a ""#, "a "),
            (r#"Link form=Markdown target="c""#, "[b](c)"),
            (r#"Text "b""#, "b"),
            (r#"Text " ""#, " "),
            (r#"Code "d""#, "`d`"),
            (r#"Text " ""#, " "),
            (r#"Link form=Wiki target="e""#, "[[e|f]]"),
            (r#"Text "f""#, "f"),
        ],
    );
}

#[test]
fn emphasis_strong_html_autolinks_and_line_breaks_span_their_delimiters() {
    let whole = "**a *b*** <i> <x@y.z>  \nc [d][1]\\\ne\n\n[1]: /u\n";
    let paragraph = "**a *b*** <i> <x@y.z>  \nc [d][1]\\\ne";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", paragraph),
            ("Paragraph", paragraph),
            ("Line", "**a *b*** <i> <x@y.z>  "),
            ("Strong", "**a *b***"),
            (r#"Text "a ""#, "a "),
            ("Emphasis", "*b*"),
            (r#"Text "b""#, "b"),
            (r#"Text " ""#, " "),
            (r#"Html "<i>""#, "<i>"),
            (r#"Text " ""#, " "),
            (r#"Link form=Markdown target="mailto:x@y.z""#, "<x@y.z>"),
            (r#"Text "x@y.z""#, "x@y.z"),
            ("LineBreak", "  "),
            ("Line", "c [d][1]\\"),
            (r#"Text "c ""#, "c "),
            (r#"Link form=Markdown target="/u""#, "[d][1]"),
            (r#"Text "d""#, "d"),
            ("LineBreak", "\\"),
            ("Line", "e"),
            (r#"Text "e""#, "e"),
        ],
    );
}

#[test]
fn yaml_frontmatter_spans() {
    let whole = "---\na: 1\nb:\n  - x\nc:\n---\nbody\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("Frontmatter format=Yaml", "---\na: 1\nb:\n  - x\nc:\n---"),
            ("Mapping", "a: 1\nb:\n  - x\nc:"),
            ("Pair key=a", "a: 1"),
            ("Scalar Integer 1", "1"),
            ("Pair key=b", "b:\n  - x"),
            ("Sequence", "- x"),
            (r#"Scalar String "x""#, "x"),
            ("Pair key=c", "c:"),
            ("Scalar Null", ""),
            ("MarkdownBlock", "body\n"),
            ("Section", "body"),
            ("Paragraph", "body"),
            ("Line", "body"),
            (r#"Text "body""#, "body"),
        ],
    );
}

#[test]
fn toml_frontmatter_spans() {
    let whole = "+++\nk = [1, \"s\"]\nt = { a = 2 }\n+++\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            (
                "Frontmatter format=Toml",
                "+++\nk = [1, \"s\"]\nt = { a = 2 }\n+++",
            ),
            ("Mapping", "k = [1, \"s\"]\nt = { a = 2 }"),
            ("Pair key=k", "k = [1, \"s\"]"),
            ("Sequence", "[1, \"s\"]"),
            ("Scalar Integer 1", "1"),
            (r#"Scalar String "s""#, "\"s\""),
            ("Pair key=t", "t = { a = 2 }"),
            ("Mapping", "{ a = 2 }"),
            ("Pair key=a", "a = 2"),
            ("Scalar Integer 2", "2"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn an_indented_code_block_spans_from_its_code_column_to_its_last_line() {
    let whole = "- a\n\n        code\n\n      more\n\n";
    let item = "- a\n\n        code\n\n      more";
    let content = "a\n\n        code\n\n      more";
    let code = "  code\n\n      more";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", item),
            ("List loose", item),
            ("Item", item),
            ("MarkdownBlock", content),
            ("Section", content),
            ("Paragraph", "a"),
            ("Line", "a"),
            (r#"Text "a""#, "a"),
            (r#"CodeBlock value="  code\n\nmore""#, code),
        ],
    );
}

#[test]
fn toml_header_dotted_key_and_multi_line_string_spans() {
    let whole = "+++\na.b = 1\ns = '''\nx'''\n[[p]]\nn = 2\n+++\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            (
                "Frontmatter format=Toml",
                "+++\na.b = 1\ns = '''\nx'''\n[[p]]\nn = 2\n+++",
            ),
            ("Mapping", "a.b = 1\ns = '''\nx'''\n[[p]]\nn = 2"),
            ("Pair key=a", "a.b = 1"),
            ("Mapping", "b = 1"),
            ("Pair key=b", "b = 1"),
            ("Scalar Integer 1", "1"),
            ("Pair key=s", "s = '''\nx'''"),
            (r#"Scalar String "x""#, "'''\nx'''"),
            ("Pair key=p", "[[p]]\nn = 2"),
            ("Sequence", "[[p]]\nn = 2"),
            ("Mapping", "n = 2"),
            ("Pair key=n", "n = 2"),
            ("Scalar Integer 2", "2"),
            ("MarkdownBlock", ""),
        ],
    );
}

#[test]
fn a_block_quote_a_break_and_html_span_their_lines_and_a_heading_its_underline() {
    let whole = "> a\n> b\n\nT\n===\n\n***\n\n<div>\nx\n</div>\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "> a\n> b"),
            (r#"BlockQuote "a\nb""#, "> a\n> b"),
            ("Section", "T\n===\n\n***\n\n<div>\nx\n</div>"),
            ("Heading", "T\n==="),
            (r#"Text "T""#, "T"),
            ("ThematicBreak", "***"),
            (r#"Html "<div>\nx\n</div>""#, "<div>\nx\n</div>"),
        ],
    );
}

#[test]
fn the_text_of_a_heading_over_two_lines_spans_both() {
    let whole = "first\n  second `code`\n---\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "first\n  second `code`\n---"),
            ("Section", "first\n  second `code`\n---"),
            ("Heading", "first\n  second `code`\n---"),
            (r#"Text "first second ""#, "first\n  second "),
            (r#"Code "code""#, "`code`"),
        ],
    );
}

#[test]
fn each_piece_of_a_node_across_lines_spans_its_part_on_its_line() {
    let whole = "Some *emphasis that\ngoes on* here.\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "Some *emphasis that\ngoes on* here."),
            ("Paragraph", "Some *emphasis that\ngoes on* here."),
            ("Line", "Some *emphasis that"),
            (r#"Text "Some ""#, "Some "),
            ("Emphasis continues=next", "*emphasis that"),
            (r#"Text "emphasis that""#, "emphasis that"),
            ("Line", "goes on* here."),
            ("Emphasis continues=previous", "goes on*"),
            (r#"Text "goes on""#, "goes on"),
            (r#"Text " here.""#, " here."),
        ],
    );
}

#[test]
fn a_piece_that_two_spaces_end_spans_its_line_break() {
    let whole = "*a  \nb*\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "*a  \nb*"),
            ("Paragraph", "*a  \nb*"),
            ("Line", "*a  "),
            ("Emphasis continues=next", "*a  "),
            (r#"Text "a""#, "a"),
            ("LineBreak", "  "),
            ("Line", "b*"),
            ("Emphasis continues=previous", "b*"),
            (r#"Text "b""#, "b"),
        ],
    );
}

#[test]
fn a_piece_of_inline_html_spans_the_spaces_before_its_line_end() {
    let whole = "x <span  \nclass=\"x\">\n";
    check_spans(
        whole,
        &[
            ("Document", whole),
            ("MarkdownBlock", whole),
            ("Section", "x <span  \nclass=\"x\">"),
            ("Paragraph", "x <span  \nclass=\"x\">"),
            ("Line", "x <span  "),
            (r#"Text "x ""#, "x "),
            (r#"Html "<span  " continues=next"#, "<span  "),
            ("Line", "class=\"x\">"),
            (r#"Html "class=\"x\">" continues=previous"#, "class=\"x\">"),
        ],
    );
}
