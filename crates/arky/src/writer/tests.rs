//! Each sample is a tree written back to markdown: read from a source, or
//! built, then written, then read again.

#[cfg(feature = "binary-format")]
use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone as _};
use indexmap::IndexMap;
#[cfg(feature = "binary-format")]
use rkyv::rancor::Error;
#[cfg(feature = "binary-format")]
use rkyv::util::AlignedVec;
#[cfg(feature = "binary-format")]
use rkyv::with::With;

#[cfg(feature = "binary-format")]
use crate::parser::ast::ArchivedRawDocument;
use crate::parser::ast::{
    Alignment, Args, Block, Code, CodeBlock, Document, Emphasis, FrontmatterFormat, Heading,
    Inline, Item, Kind, Line, LineBreak, Link, LinkForm, LinkInline, LinkStrong, List, Mapping,
    MarkdownBlock, Paragraph, RawDocument, Section, Strong, Table, TableCell, TableFormat,
    TableRow, Text, Value,
};
use crate::parser::error::{ParseError, RebuildError};
use crate::parser::extension::{Construct, Context, Extension, Markdown, Plain, Written};
#[cfg(feature = "binary-format")]
use crate::parser::frontmatter::VALUE_CAP;
#[cfg(feature = "binary-format")]
use crate::parser::inline::INLINE_CAP;
#[cfg(feature = "binary-format")]
use crate::parser::lexer::BLOCK_CAP;
#[cfg(feature = "binary-format")]
use crate::parser::mirror::RawDocumentMirror;
#[cfg(feature = "binary-format")]
use crate::parser::tests::blocks::nested_grid_tables;
#[cfg(feature = "binary-format")]
use crate::parser::tests::json::unclaim;
#[cfg(feature = "binary-format")]
use crate::parser::tests::on_two_megabytes;
use crate::parser::{ParseOptions, Parsed, parse, parse_with, rebuild_with};
#[cfg(feature = "binary-format")]
use crate::writer::write_archived;
use crate::writer::{WriteError, WriteOptions, Writer, write};

/// Every file under `parser/tests/fixtures/`.
const FIXTURES: [(&str, &str); 21] = [
    (
        "api_reference",
        include_str!("../parser/tests/fixtures/api_reference.md"),
    ),
    (
        "book_page",
        include_str!("../parser/tests/fixtures/book_page.md"),
    ),
    (
        "changelog",
        include_str!("../parser/tests/fixtures/changelog.md"),
    ),
    (
        "contact",
        include_str!("../parser/tests/fixtures/contact.md"),
    ),
    (
        "daily_note",
        include_str!("../parser/tests/fixtures/daily_note.md"),
    ),
    (
        "docusaurus_page",
        include_str!("../parser/tests/fixtures/docusaurus_page.md"),
    ),
    ("essay", include_str!("../parser/tests/fixtures/essay.md")),
    (
        "hugo_post",
        include_str!("../parser/tests/fixtures/hugo_post.md"),
    ),
    (
        "hugo_post_with_table",
        include_str!("../parser/tests/fixtures/hugo_post_with_table.md"),
    ),
    (
        "issue_template",
        include_str!("../parser/tests/fixtures/issue_template.md"),
    ),
    (
        "jekyll_post",
        include_str!("../parser/tests/fixtures/jekyll_post.md"),
    ),
    (
        "meeting_notes",
        include_str!("../parser/tests/fixtures/meeting_notes.md"),
    ),
    (
        "obsidian_note",
        include_str!("../parser/tests/fixtures/obsidian_note.md"),
    ),
    (
        "pandoc_paper",
        include_str!("../parser/tests/fixtures/pandoc_paper.md"),
    ),
    ("readme", include_str!("../parser/tests/fixtures/readme.md")),
    ("recipe", include_str!("../parser/tests/fixtures/recipe.md")),
    (
        "slash_command",
        include_str!("../parser/tests/fixtures/slash_command.md"),
    ),
    ("task", include_str!("../parser/tests/fixtures/task.md")),
    (
        "toml_multi_line_string",
        include_str!("../parser/tests/fixtures/toml_multi_line_string.md"),
    ),
    (
        "yaml_list_of_mappings",
        include_str!("../parser/tests/fixtures/yaml_list_of_mappings.md"),
    ),
    (
        "zola_page",
        include_str!("../parser/tests/fixtures/zola_page.md"),
    ),
];

/// The owner's sample of arky's written form.
const OWNERS_SAMPLE: &str = "---
title: 'Notes'
tags: [a, b]
---
Notes
=====

Some __bold__ and _em_ text.

* one
*   two
    continued

~~~python
print(1)
~~~

___
";

/// TOML frontmatter with a table, then a heading.
const TOML_SAMPLE: &str =
    "+++\ntitle = \"Notes\"\ndue = 2026-10-10\n[params]\nauthor = \"Ada\"\n+++\n# Notes\n";

/// Sources of the constructs, and the guards around them, that the fixture
/// files do not hold.
#[cfg(feature = "binary-format")]
const CONSTRUCTS: [&str; 28] = [
    "Title\n=====\n\nSub\n---\ntext\n\n# A\n\n#\n\nfoo\n",
    "Setext a  \nb\n===\n",
    "a\\\n    <div>\n===\n",
    "\\[a]: b  \nc\n===\n",
    "- a\n* b\n+ c\n\n1. a\n1) b\n",
    "3. a\n\n4. b\n\n- a\n\n  second\n- b\n",
    "-\n  - -\n\n-\n  \t<div>\n\n- a\n+ ***\n",
    "- ```\n  a\n\n  b\n  ```\n",
    "_a_ __b__ `c` ``a`b`` *a*_b_ __*a*__ _*a*_\n",
    "*a `b`* **c `d`** [e `f`](u) ```g``h```\n",
    "[a\\[b](u) [*c\\]d*](u)\n",
    "Some *emphasis that\ngoes on* here.\nA [link that\ngoes on\nand on](u \"t\") here.\n",
    "a ``code ` that\ngoes on`` b\n\na ``b\n    # c``\n",
    "[g][1] [a](<b c> 'd') <https://x.org> [a](u(1)) [a](<u)>) [a](u&#10;x)\n\n[1]: u\n",
    "[[t|x]] [[only]] ![a](i.png) ![[i.png|alt]] ![[i.png]] ![a](<b c> \"t\")\n",
    "a &amp;amp; b &#38;#35; c & d &#32;\n\n\\*a\\* \\_b\\_ \\`c\\` \\[d](e) \\<f> \\!\\[g](h)\n",
    "\\# a\n\n\\> b\n\n\\- c\n\n1\\. d\n\na\n\\===\n\n#tag and [x] here\n",
    "a  \nb\\\nc\n\n>a\n>\n>  b\n\n<div>\n  <p>x</p>\n</div>\n\na <span>b</span>\n\n***\n",
    "|a|bb|\n|-|:-|\n|1|2|\n\n| a \\| b |\n|---|\n",
    "+-----+-----+\n| a   | b   |\n+=====+====:+\n| one | x   |\n|     |     |\n| two |     |\n+-----+-----+\n",
    "a   b\n--- ---\n1   2\n",
    "\tcode\n\n~~~python\nprint(1)\n~~~\n\n````\n```\ninside\n```\n````\n\n```\n```\n",
    "```md title=\"README.md\",showLineNumbers=true\nx\n```\n\n```md a.b=1,a.c=2,d={}\nx\n```\n",
    "```md a=1} # x\nx\n```\n\n```Markdown title\n# A\n```\n",
    "a \\| b\n\\--- \\| ---\n\n\\+---+\n\\| a \\|\n\\+---+\n",
    "\\+++\nx = 1\n\\+++\n",
    "\u{feff}\u{feff}# A\n",
    "a\n    ```b\n    c``d```\n",
];

/// Frontmatter written in TOML.
const TOML: WriteOptions = WriteOptions {
    frontmatter: FrontmatterFormat::Toml,
};

/// Claims every heading and writes it in its plain form.
#[derive(Debug, Clone, PartialEq)]
struct Headings;

impl Extension for Headings {
    type Node<T>
        = ()
    where
        T: Extension;

    fn claim<T>(
        construct: Construct<'_, T>,
        _context: &Context<'_>,
    ) -> Option<Result<(), ParseError>>
    where
        T: Extension,
    {
        matches!(construct, Construct::Heading(_)).then_some(Ok(()))
    }

    fn write<T>(_node: &(), plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension,
    {
        writer.construct(plain);
    }
}

fn written(source: &str) -> String {
    write(&parse(source).document, &WriteOptions::default()).unwrap()
}

/// The document `parsed` holds, a valid one.
fn valid<E>(parsed: Parsed<E>) -> Document<E>
where
    E: Extension,
{
    let read = match parsed.document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => Some(valid),
    };
    read.unwrap()
}

/// Writes `document`, a valid one.
fn write_valid<E>(document: &Document<E>, options: &WriteOptions) -> Result<String, WriteError>
where
    E: Extension,
{
    write(&RawDocument::Valid(document.clone()), options)
}

/// The body `source` reads as, every table in pipe syntax.
fn body(source: &str) -> MarkdownBlock {
    let mut read = valid(parse(source)).body;
    clear_block(&mut read);
    read
}

/// Writes `source`, reads the text back, and checks that it reads as the
/// same body and writes as the same text again. Gives the text.
fn round_trip(source: &str) -> String {
    let text = written(source);
    assert_eq!(body(&text), body(source), "{source:?} became {text:?}");
    assert_eq!(written(&text), text, "{source:?} became {text:?}");
    text
}

fn clear_block(block: &mut MarkdownBlock) {
    clear_kind(&mut block.kind);
    for section in &mut block.sections {
        clear_section(section);
    }
}

fn clear_kind(kind: &mut Kind<Markdown>) {
    match kind.0.as_deref_mut() {
        Some(RawDocument::Invalid(invalid)) => clear_block(&mut invalid.body),
        Some(RawDocument::Valid(valid)) => clear_block(&mut valid.body),
        None => {}
    }
}

fn clear_section(section: &mut Section) {
    clear_kind(&mut section.kind);
    if let Some(heading) = &mut section.heading {
        clear_kind(&mut heading.kind);
        clear_inlines(&mut heading.children);
    }
    for block in &mut section.blocks {
        clear_block_node(block);
    }
    for subsection in &mut section.subsections {
        clear_section(subsection);
    }
}

fn clear_block_node(block: &mut Block) {
    match block {
        Block::BlockQuote(quote) => {
            clear_kind(&mut quote.kind);
        }
        Block::CodeBlock(code) => {
            clear_kind(&mut code.kind);
        }
        Block::Html(html) => {
            clear_kind(&mut html.kind);
        }
        Block::List(list) => {
            clear_kind(&mut list.kind);
            for item in &mut list.items {
                clear_kind(&mut item.kind);
                clear_block(&mut item.body);
            }
        }
        Block::Paragraph(paragraph) => {
            clear_kind(&mut paragraph.kind);
            for line in &mut paragraph.lines {
                clear_kind(&mut line.kind);
                clear_inlines(&mut line.children);
            }
        }
        Block::Table(table) => clear_table(table),
        Block::ThematicBreak(thematic_break) => {
            clear_kind(&mut thematic_break.kind);
        }
    }
}

fn clear_table(table: &mut Table) {
    table.format = TableFormat::Pipe;
    clear_kind(&mut table.kind);
    for row in table.head.iter_mut().chain(&mut table.rows) {
        clear_kind(&mut row.kind);
        for cell in &mut row.cells {
            clear_kind(&mut cell.kind);
            clear_block(&mut cell.body);
        }
    }
}

fn clear_inlines(inlines: &mut [Inline]) {
    for inline in inlines {
        match inline {
            Inline::Code(code) => {
                clear_kind(&mut code.kind);
            }
            Inline::Emphasis(emphasis) => {
                clear_kind(&mut emphasis.kind);
                clear_inlines(&mut emphasis.children);
            }
            Inline::Html(html) => {
                clear_kind(&mut html.kind);
            }
            Inline::Image(image) => {
                clear_kind(&mut image.kind);
            }
            Inline::LineBreak(line_break) => {
                clear_kind(&mut line_break.kind);
            }
            Inline::Link(link) => {
                clear_kind(&mut link.kind);
                clear_link_text(&mut link.children);
            }
            Inline::Strong(strong) => {
                clear_kind(&mut strong.kind);
                clear_inlines(&mut strong.children);
            }
            Inline::Text(text) => {
                clear_kind(&mut text.kind);
            }
        }
    }
}

fn clear_link_text(inlines: &mut [LinkInline]) {
    for inline in inlines {
        match inline {
            LinkInline::Code(code) => {
                clear_kind(&mut code.kind);
            }
            LinkInline::Emphasis(emphasis) => {
                clear_kind(&mut emphasis.kind);
                clear_link_text(&mut emphasis.children);
            }
            LinkInline::Html(html) => {
                clear_kind(&mut html.kind);
            }
            LinkInline::Image(image) => {
                clear_kind(&mut image.kind);
            }
            LinkInline::LineBreak(line_break) => {
                clear_kind(&mut line_break.kind);
            }
            LinkInline::Strong(strong) => {
                clear_kind(&mut strong.kind);
                clear_link_text(&mut strong.children);
            }
            LinkInline::Text(text) => {
                clear_kind(&mut text.kind);
            }
        }
    }
}

fn text_node(value: &str) -> Inline {
    Inline::Text(Text {
        kind: Kind(None),
        value: value.to_owned(),
    })
}

/// A section headed `title`, holding a paragraph `text`, then
/// `subsections`.
fn titled(title: &str, text: &str, subsections: Vec<Section>) -> Section {
    Section {
        blocks: vec![Block::Paragraph(Paragraph {
            kind: Kind(None),
            lines: vec![Line {
                children: vec![text_node(text)],
                kind: Kind(None),
            }],
        })],
        heading: Some(Heading {
            children: vec![text_node(title)],
            kind: Kind(None),
        }),
        kind: Kind(None),
        subsections,
    }
}

/// A section with no blocks, headed by `children`, then `subsections`.
fn headed(children: Vec<Inline>, subsections: Vec<Section>) -> Section {
    Section {
        blocks: Vec::new(),
        heading: Some(Heading {
            children,
            kind: Kind(None),
        }),
        kind: Kind(None),
        subsections,
    }
}

/// Sections `Level from` to `Level to`, each inside the one before it.
fn levels(from: usize, to: usize) -> Section {
    let subsections = if from < to {
        vec![levels(from + 1, to)]
    } else {
        Vec::new()
    };
    titled(
        &format!("Level {from}"),
        &format!("text {from}"),
        subsections,
    )
}

fn document(sections: Vec<Section>) -> Document {
    Document {
        body: MarkdownBlock {
            kind: Kind(None),
            sections,
        },
        frontmatter: None,
        kind: Kind(None),
    }
}

/// `section` with each `markdown` block that ends its blocks, the form a
/// section seven levels deep is written in, put back among its
/// subsections.
fn lifted(mut section: Section) -> Section {
    let mut deeper = Vec::new();
    while let Some(Block::CodeBlock(code)) = section.blocks.last()
        && code.language.as_deref() == Some("markdown")
        && let Some(Block::CodeBlock(claimed)) = section.blocks.pop()
        && let Some(nested) = claimed.kind.0
        && let RawDocument::Valid(document) = *nested
    {
        let mut sections = document.body.sections;
        sections.append(&mut deeper);
        deeper = sections;
    }
    section.subsections.append(&mut deeper);
    section.subsections = section.subsections.into_iter().map(lifted).collect();
    section
}

#[test]
fn every_fixture_reads_back_to_the_same_body_and_writes_the_same_text_again() {
    for (name, source) in FIXTURES {
        let text = written(source);
        assert_eq!(body(&text), body(source), "{name}: {text}");
        assert_eq!(written(&text), text, "{name}");
    }
}

#[test]
fn the_owners_sample_is_written_in_arkys_form() {
    let source = "\
Notes
=====

Some __bold__ and _em_ text.

* one
*   two
    continued

~~~python
print(1)
~~~

___
";
    assert_eq!(
        round_trip(source),
        "\
# Notes

Some **bold** and *em* text.

- one
- two
  continued

```python
print(1)
```

***
"
    );
}

#[test]
fn a_thematic_break_of_any_marks_is_written_as_stars() {
    assert_eq!(round_trip("a\n\n___\n\n* * *\n"), "a\n\n***\n\n***\n");
}

#[test]
fn every_heading_line_is_written_and_a_section_with_none_writes_no_line() {
    assert_eq!(
        round_trip("# Hi H1\n### This is H3\n"),
        "# Hi H1\n\n### This is H3\n"
    );
    assert_eq!(round_trip("intro\n"), "intro\n");
    assert_eq!(round_trip("intro\n## B\n"), "intro\n\n## B\n");
    assert_eq!(round_trip("## Intro\n\n# Title\n"), "## Intro\n\n# Title\n");
    assert_eq!(round_trip("Sub\n---\ntext\n"), "## Sub\n\ntext\n");
}

#[test]
fn a_heading_line_with_no_text_keeps_its_section() {
    assert_eq!(round_trip("# A\n\n#\n\nfoo\n"), "# A\n\n#\n\nfoo\n");
    assert_eq!(
        round_trip("# Hi H1\n##\n### This is H3\n"),
        "# Hi H1\n\n##\n\n### This is H3\n"
    );
}

#[test]
fn a_heading_is_written_without_its_closing_hashes_and_keeps_the_hashes_of_its_text() {
    for (source, text) in [
        ("# Title ##\n", "# Title\n"),
        ("### Title ###   \n", "### Title\n"),
        ("# #\n", "#\n"),
        ("### ###\n", "###\n"),
        ("# Title#\n", "# Title#\n"),
        ("# Title \\#\n", "# Title \\#\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_built_heading_whose_text_ends_with_hashes_after_a_space_escapes_the_first_hash() {
    let built = document(vec![headed(vec![text_node("Title ##")], Vec::new())]);
    let text = write_valid(&built, &WriteOptions::default()).unwrap();
    assert_eq!(text, "# Title \\##\n");
    assert_eq!(body(&text), built.body);
    assert_eq!(written(&text), text);
}

#[test]
fn a_section_with_no_heading_after_a_sibling_writes_an_empty_heading_line() {
    let untitled = |section: Section| Section {
        heading: None,
        ..section
    };
    let a = || headed(vec![text_node("A")], Vec::new());
    let b = || headed(vec![text_node("B")], Vec::new());
    for (sections, text) in [
        (
            vec![a(), untitled(titled("", "foo", Vec::new()))],
            "# A\n\n#\n\nfoo\n",
        ),
        (
            vec![a(), untitled(headed(Vec::new(), vec![b()]))],
            "# A\n\n#\n\n## B\n",
        ),
        (
            vec![untitled(titled("", "intro", Vec::new())), b()],
            "intro\n\n# B\n",
        ),
    ] {
        let built = document(sections);
        let written_once = write_valid(&built, &WriteOptions::default()).unwrap();
        assert_eq!(written_once, text);
        assert_eq!(written(text), text);
        let mut read_back = built.body;
        for section in read_back.sections.iter_mut().skip(1) {
            if section.heading.is_none() {
                section.heading = headed(Vec::new(), Vec::new()).heading;
            }
        }
        assert_eq!(body(text), read_back, "{text:?}");
    }
}

#[test]
fn a_first_section_with_no_heading_that_a_read_would_not_build_writes_an_empty_heading_line() {
    let trees = |heading: Option<Heading>| {
        let a = |subsections| headed(vec![text_node("A")], subsections);
        let foo = Section {
            heading: heading.clone(),
            ..titled("", "foo", Vec::new())
        };
        let empty = || Section {
            heading: heading.clone(),
            ..headed(Vec::new(), Vec::new())
        };
        [
            vec![a(vec![foo])],
            vec![a(vec![empty()])],
            vec![empty(), headed(vec![text_node("B")], Vec::new())],
        ]
    };
    let texts = ["# A\n\n##\n\nfoo\n", "# A\n\n##\n", "#\n\n# B\n"];
    let read_backs = trees(headed(Vec::new(), Vec::new()).heading);
    for ((sections, read_back), text) in trees(None).into_iter().zip(read_backs).zip(texts) {
        let written_once = write_valid(&document(sections), &WriteOptions::default()).unwrap();
        assert_eq!(written_once, text);
        assert_eq!(written(text), text);
        assert_eq!(body(text), document(read_back).body, "{text:?}");
    }
}

#[test]
fn a_section_seven_levels_deep_is_a_markdown_block_inside_its_parent() {
    let text = write_valid(&document(vec![levels(1, 7)]), &WriteOptions::default()).unwrap();
    assert!(
        text.ends_with("###### Level 6\n\ntext 6\n\n```markdown\n# Level 7\n\ntext 7\n```\n"),
        "{text}"
    );
    let source = "###### F\n\n````markdown\n# G\n\n```rust\nfn main() {}\n```\n````\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn a_section_twenty_two_levels_deep_reads_back_to_the_same_tree() {
    let built = document(vec![levels(1, 22)]);
    let text = write_valid(&built, &WriteOptions::default()).unwrap();
    let fences: Vec<&str> = text.lines().filter(|line| line.starts_with('`')).collect();
    assert_eq!(
        fences,
        [
            "`````markdown",
            "````markdown",
            "```markdown",
            "```",
            "````",
            "`````"
        ]
    );
    assert_eq!(written(&text), text);
    let read = valid(parse(&text)).body;
    let mut read_back = MarkdownBlock {
        sections: read.sections.into_iter().map(lifted).collect(),
        ..read
    };
    clear_block(&mut read_back);
    assert_eq!(read_back, built.body);
}

#[test]
fn lists_are_written_with_dashes_and_numbers_counted_up_from_the_first() {
    assert_eq!(round_trip("3. a\n\n4. b\n"), "3. a\n\n4. b\n");
    assert_eq!(round_trip("1) a\n1) b\n"), "1. a\n2. b\n");
    assert_eq!(
        round_trip("+ a\n+ b\n  + c\n    more\n"),
        "- a\n- b\n  - c\n    more\n"
    );
    assert_eq!(
        round_trip("9. a\n10. b\n    more\n"),
        "9. a\n10. b\n    more\n"
    );
    assert_eq!(
        round_trip("- a\n\n  second\n- b\n"),
        "- a\n\n  second\n\n- b\n"
    );
    assert_eq!(round_trip("- # A\n  text\n- b\n"), "- # A\n  text\n- b\n");
    assert_eq!(round_trip("-\n- b\n"), "-\n- b\n");
}

#[test]
fn a_line_left_of_the_item_text_is_written_as_a_paragraph_after_the_list() {
    for source in ["- a\nb\n", "- a\n b\n"] {
        assert_eq!(written(source), "- a\n\nb\n", "{source:?}");
        assert_eq!(written(&written(source)), "- a\n\nb\n", "{source:?}");
    }
}

#[test]
fn emphasis_strong_and_code_take_arkys_marks_and_fences() {
    assert_eq!(round_trip("_a_ __b__ `c`\n"), "*a* **b** `c`\n");
    assert_eq!(round_trip("``a`b``\n"), "``a`b``\n");
    assert_eq!(
        round_trip("````\n```\ninside\n```\n````\n"),
        "````\n```\ninside\n```\n````\n"
    );
    assert_eq!(
        round_trip("text\n\n    indented\n\n    code\n"),
        "text\n\n```\nindented\n\ncode\n```\n"
    );
    assert_eq!(round_trip("```\n```\n"), "```\n```\n");
    assert_eq!(
        round_trip("- ```\n  a\n\n  b\n  ```\n"),
        "- ```\n  a\n\n  b\n  ```\n"
    );
}

#[test]
fn links_are_written_inline_and_wiki_links_and_embeds_keep_their_form() {
    assert_eq!(round_trip("[g][1]\n\n[1]: u\n"), "[g](u)\n");
    assert_eq!(
        round_trip("[a](<b c> 'd') <https://x.org>\n"),
        "[a](<b c> \"d\") [https://x.org](https://x.org)\n"
    );
    assert_eq!(
        round_trip("[[t|x]] [[only]] ![a](i.png) ![[i.png|alt]] ![[i.png]]\n"),
        "[[t|x]] [[only]] ![a](i.png) ![[i.png|alt]] ![[i.png]]\n"
    );
    assert_eq!(round_trip("[a](u(1))\n"), "[a](u(1))\n");
    assert_eq!(round_trip("[a](<u)>)\n"), "[a](<u)>)\n");
}

#[test]
fn a_node_cut_over_several_lines_is_written_once_over_the_same_lines() {
    assert_eq!(
        round_trip("Some *emphasis that\ngoes on* here.\n"),
        "Some *emphasis that\ngoes on* here.\n"
    );
    assert_eq!(
        round_trip("A [link that\ngoes on\nand on](u \"t\") here.\n"),
        "A [link that\ngoes on\nand on](u \"t\") here.\n"
    );
    assert_eq!(
        round_trip("a `code that\ngoes on` b\n"),
        "a `code that\ngoes on` b\n"
    );
    assert_eq!(
        round_trip("a ``code ` that\ngoes on`` b\n"),
        "a ``code ` that\ngoes on`` b\n"
    );
    assert_eq!(round_trip("one\ntwo\n"), "one\ntwo\n");
}

#[test]
fn an_ampersand_that_would_read_as_a_reference_is_escaped() {
    assert_eq!(
        round_trip("a &amp;amp; b &#38;#35; c & d\n"),
        "a \\&amp; b \\&#35; c & d\n"
    );
}

#[test]
fn a_code_span_that_the_reader_would_strip_is_padded() {
    assert_eq!(round_trip("`` `a` ``\n"), "`` `a` ``\n");
    assert_eq!(round_trip("`  a  `\n"), "`  a  `\n");
    assert_eq!(round_trip("` `\n"), "` `\n");
    assert_eq!(round_trip("`` a` ``\n"), "`` a` ``\n");
}

#[test]
fn text_that_would_read_as_markup_is_escaped() {
    assert_eq!(
        round_trip("\\*a\\* \\_b\\_ \\`c\\` \\[d](e) \\<f> \\!\\[g](h)\n"),
        "\\*a\\* \\_b\\_ \\`c\\` [d\\](e) \\<f> \\![g\\](h)\n"
    );
    assert_eq!(
        round_trip("snake_case a * b C:\\dir\n"),
        "snake_case a * b C:\\dir\n"
    );
    assert_eq!(
        round_trip("\\# a\n\n\\> b\n\n\\- c\n\n1\\. d\n"),
        "\\# a\n\n\\> b\n\n\\- c\n\n1\\. d\n"
    );
    assert_eq!(round_trip("a\n\\===\n"), "a\n\\===\n");
    assert_eq!(round_trip("\\[a]: b\n"), "\\[a]: b\n");
    assert_eq!(round_trip("#tag and [x] here\n"), "#tag and [x] here\n");
    assert_eq!(
        round_trip("| a \\| b |\n|---|\n"),
        "| a \\| b |\n| ------ |\n"
    );
}

#[test]
fn blocks_that_tabs_indent_or_separate_are_written_in_arkys_form() {
    for (source, text) in [
        ("\tcode\n", "```\ncode\n```\n"),
        ("#\tTitle\n", "# Title\n"),
        ("-\titem\n", "- item\n"),
        ("*\t*\t*\n", "***\n"),
        ("- foo\n\n\tbar\n", "- foo\n\n  bar\n"),
        ("- foo\n\n\t\tbar\n", "- foo\n\n  ```\n    bar\n  ```\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_text_line_that_a_tab_would_make_a_block_is_escaped() {
    for text in ["\\-\tx\n", "\\#\tx\n", "1\\.\tx\n", "\\*\t*\t*\n"] {
        assert_eq!(round_trip(text), text);
    }
}

#[test]
fn an_item_whose_first_line_starts_with_a_tab_writes_it_under_the_marker() {
    assert_eq!(round_trip("-\n  \t<div>\n"), "-\n    <div>\n");
}

#[test]
fn an_item_whose_marker_line_would_read_as_a_thematic_break_writes_its_first_line_under_it() {
    for source in ["-\n  - -\n", "-\t- *\n"] {
        assert_eq!(round_trip(source), "-\n  - -\n", "{source:?}");
    }
}

#[test]
fn an_html_block_led_by_a_tab_reads_back_the_same_under_a_wider_marker() {
    let source = format!("{}1. j\n\n   \t<div>\n", "1. a\n".repeat(9));
    assert_eq!(
        round_trip(&source),
        "1. a\n\n2. a\n\n3. a\n\n4. a\n\n5. a\n\n6. a\n\n7. a\n\n8. a\n\n9. a\n\n10. j\n\n     <div>\n"
    );
}

#[test]
fn a_text_line_that_would_open_a_tilde_fence_is_escaped() {
    assert_eq!(round_trip("\\~~~\n"), "\\~~~\n");
    assert_eq!(round_trip("a\n\\~~~ b\n"), "a\n\\~~~ b\n");
    assert_eq!(round_trip("~~\n"), "~~\n");
}

#[test]
fn a_later_line_led_by_inline_html_or_a_code_span_piece_that_would_start_a_block_is_indented() {
    for (source, text) in [
        ("a\n    <div>\n", "a\n    <div>\n"),
        ("a\n\t<div>\n", "a\n    <div>\n"),
        ("a ``b\n    # c``\n", "a `b\n    # c`\n"),
        ("a ``b\n\t# c``\n", "a `b\n    # c`\n"),
        ("a\n    ```b\n    c``d```\n", "a\n    ```b\nc``d```\n"),
        ("a\n\t```b\n\tc``d```\n", "a\n    ```b\nc``d```\n"),
        ("a\n    ````b\n    c```d````\n", "a\n    ````b\nc```d````\n"),
        ("a\\\n    <div>\n===\n", "a\\\n    <div>\n===\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_first_body_line_that_would_open_a_toml_frontmatter_is_escaped() {
    let text = round_trip("\\+++\nx = 1\n\\+++\n");
    assert_eq!(text, "\\+++\nx = 1\n+++\n");
    assert!(valid(parse(&text)).frontmatter.is_none());
    assert_eq!(round_trip("+++ x\n"), "+++ x\n");
    assert_eq!(round_trip("\\---\nx: 1\n\\---\n"), "\\---\nx: 1\n\\---\n");
}

#[test]
fn a_paragraph_that_would_read_as_a_table_escapes_its_pipes() {
    assert_eq!(
        round_trip("a \\| b\n\\--- \\| ---\n"),
        "a \\| b\n--- \\| ---\n"
    );
    assert_eq!(
        round_trip("\\+---+\n\\| a \\|\n\\+---+\n"),
        "+---+\n\\| a \\|\n+---+\n"
    );
    assert_eq!(round_trip("a | b\n"), "a | b\n");
}

#[test]
fn line_breaks_quotes_html_and_breaks_take_arkys_form() {
    assert_eq!(round_trip("a  \nb\\\nc\n"), "a\\\nb\\\nc\n");
    assert_eq!(round_trip(">a\n>\n>  b\n"), "> a\n>\n>  b\n");
    assert_eq!(
        round_trip("<div>\n  <p>x</p>\n</div>\n"),
        "<div>\n  <p>x</p>\n</div>\n"
    );
    assert_eq!(round_trip("a <span>b</span>\n"), "a <span>b</span>\n");
    assert_eq!(round_trip("***\n"), "***\n");
}

#[test]
fn a_table_is_a_padded_pipe_table_or_a_grid_table() {
    assert_eq!(
        round_trip("|a|bb|\n|-|:-|\n|1|2|\n"),
        "| a   | bb  |\n| --- | :-- |\n| 1   | 2   |\n"
    );
    assert_eq!(
        round_trip("| a | b | c |\n|:-:|--:|---|\n| long cell | | x |\n"),
        "| a         | b   | c   |\n| :-------: | --: | --- |\n| long cell |     | x   |\n"
    );
    let grid = "\
+-----+-----+
| a   | b   |
+=====+====:+
| one | x   |
|     |     |
| two |     |
+-----+-----+
";
    assert_eq!(round_trip(grid), grid);
}

#[test]
fn blocks_are_separated_by_one_blank_line_and_the_text_ends_with_one_newline() {
    assert_eq!(
        round_trip("# A\ntext\n- x\n> q\n\n\n\nlast"),
        "# A\n\ntext\n\n- x\n\n> q\n\nlast\n"
    );
    assert_eq!(round_trip(""), "");
}

#[test]
fn each_file_writes_the_same_text_the_second_time_and_keeps_its_tree() {
    for (source, text) in [
        ("- a\n* b\n", "- a\n\n* b\n"),
        ("- a\n* b\n+ c\n", "- a\n\n* b\n\n- c\n"),
        ("1. a\n1) b\n", "1. a\n\n1) b\n"),
        ("- a\n  ***\n- b\n", "- a\n  ***\n- b\n"),
        ("***\n\ntext\n\n***\n", "***\n\ntext\n\n***\n"),
        ("&#32;a\n", "&#32;a\n"),
        ("a&#32;\n", "a&#32;\n"),
        ("&#9;a\n", "&#9;a\n"),
        ("[a](u \"t&#10;x\")\n", "[a](u \"t&#10;x\")\n"),
        ("a&#10;b\n", "a&#10;b\n"),
        ("Setext a  \nb\n===\n", "Setext a\\\nb\n===\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn an_underlined_heading_takes_the_guards_of_a_paragraph() {
    for (source, text) in [
        ("\\[a]: b  \nc\n===\n", "\\[a]: b\\\nc\n===\n"),
        (
            "a \\| b  \n--- \\| ---\n===\n",
            "a \\| b\\\n--- \\| ---\n===\n",
        ),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_star_item_opening_with_a_thematic_break_and_a_destination_with_a_line_ending_keep_their_tree()
{
    for (source, text) in [
        ("- a\n+ ***\n", "- a\n\n* ___\n"),
        ("[a](<u&#10;x>)\n", "[a](<u&#10;x>)\n"),
        ("[a](u&#10;x)\n", "[a](<u&#10;x>)\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_heading_with_a_line_break_after_text_in_a_tight_item_is_written_on_one_line() {
    let mut built = valid(parse("- p\n  # h\n- q\n"));
    if let Some(Block::List(list)) = built.body.sections[0].blocks.first_mut()
        && let Some(heading) = &mut list.items[0].body.sections[1].heading
    {
        heading
            .children
            .push(Inline::LineBreak(LineBreak { kind: Kind(None) }));
        heading.children.push(text_node("b"));
    }
    let text = write_valid(&built, &WriteOptions::default()).unwrap();
    assert_eq!(text, "- p\n  # h&#10;b\n- q\n");
    assert_eq!(written(&text), text);
}

#[test]
fn a_claimed_heading_with_a_line_break_after_text_in_a_tight_item_is_written_on_one_line() {
    let options = ParseOptions::default();
    let mut built = valid(parse_with::<Headings>("- p\n  # h\n- q\n", &options).unwrap());
    if let Some(Block::List(list)) = built.body.sections[0].blocks.first_mut()
        && let Some(heading) = &mut list.items[0].body.sections[1].heading
    {
        heading
            .children
            .push(Inline::LineBreak(LineBreak { kind: Kind(None) }));
        heading.children.push(Inline::Text(Text {
            kind: Kind(None),
            value: "b".to_owned(),
        }));
    }
    let text = write_valid(&built, &WriteOptions::default()).unwrap();
    assert_eq!(text, "- p\n  # h&#10;b\n- q\n");
    let again = parse_with::<Headings>(&text, &options).unwrap().document;
    assert_eq!(write(&again, &WriteOptions::default()).unwrap(), text);
}

#[test]
fn touching_spans_of_one_kind_are_written_as_one_span() {
    for (source, text) in [("*a*_b_\n", "*ab*\n"), ("**a**__b__\n", "**ab**\n")] {
        assert_eq!(written(source), text, "{source:?}");
        assert_eq!(written(text), text, "{source:?}");
    }
}

#[test]
fn strong_around_emphasis_and_emphasis_inside_emphasis_keep_their_form() {
    assert_eq!(written("__*a*__\n"), "***a***\n");
    assert_eq!(written("_*a*_\n"), "**a**\n");
}

/// A document of one paragraph of one line holding `children`.
fn line_of(children: Vec<Inline>) -> Document {
    document(vec![Section::new(
        None,
        vec![Block::Paragraph(Paragraph::new(vec![Line::new(children)]))],
        Vec::new(),
    )])
}

/// Writes `built`, checks the text is `expected`, reads back as `built` and
/// writes as itself again.
fn writes_and_reads_back(built: &Document, expected: &str) {
    let text = write_valid(built, &WriteOptions::default()).unwrap();
    assert_eq!(text, expected);
    assert_eq!(body(&text), built.body);
    assert_eq!(written(&text), text);
}

#[test]
fn a_mark_between_a_letter_and_punctuation_writes_the_letter_as_a_reference() {
    let strong = |children| Inline::Strong(Strong::new(children));
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            strong(vec![text_node("(bold)")]),
            text_node("b"),
        ]),
        "&#97;**(bold)**&#98;\n",
    );
    writes_and_reads_back(
        &line_of(vec![strong(vec![text_node("bold.")]), text_node("b")]),
        "**bold.**&#98;\n",
    );
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            Inline::Emphasis(Emphasis::new(vec![text_node("(it)")])),
            text_node("b"),
        ]),
        "&#97;*(it)*&#98;\n",
    );
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            strong(vec![Inline::Code(Code::new("x".to_owned()))]),
            text_node("b"),
        ]),
        "&#97;**`x`**&#98;\n",
    );
}

#[test]
fn a_mark_with_a_space_inside_writes_the_space_and_the_letter_outside_as_references() {
    let strong = |children| Inline::Strong(Strong::new(children));
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            strong(vec![text_node(" bold")]),
            text_node("b"),
        ]),
        "&#97;**&#32;bold**b\n",
    );
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            strong(vec![text_node(" bold ")]),
            text_node("b"),
        ]),
        "&#97;**&#32;bold&#32;**&#98;\n",
    );
}

#[test]
fn a_mark_inside_a_link_s_text_writes_its_neighbours_as_references() {
    let link = Link::new(
        LinkForm::Markdown,
        "https://example.com".to_owned(),
        None,
        vec![
            LinkInline::Text(Text::new("a".to_owned())),
            LinkInline::Strong(LinkStrong::new(vec![LinkInline::Text(Text::new(
                "(bold)".to_owned(),
            ))])),
            LinkInline::Text(Text::new("b".to_owned())),
        ],
    );
    writes_and_reads_back(
        &line_of(vec![Inline::Link(link)]),
        "[&#97;**(bold)**&#98;](https://example.com)\n",
    );
}

#[test]
fn marks_that_open_and_close_where_they_stand_are_written_as_before() {
    writes_and_reads_back(
        &line_of(vec![
            text_node("a "),
            Inline::Strong(Strong::new(vec![text_node("bold")])),
            text_node(" b"),
        ]),
        "a **bold** b\n",
    );
    writes_and_reads_back(
        &line_of(vec![
            Inline::Strong(Strong::new(vec![text_node("x")])),
            Inline::Emphasis(Emphasis::new(vec![text_node("y")])),
        ]),
        "**x***y*\n",
    );
    assert_eq!(
        round_trip("&#97;**(bold)**&#98;\n"),
        "&#97;**(bold)**&#98;\n"
    );
}

#[test]
fn a_run_that_closes_a_mark_and_opens_the_next_opens_and_closes_where_it_stands() {
    let strong = |value: &str| Inline::Strong(Strong::new(vec![text_node(value)]));
    let emphasis = |value: &str| Inline::Emphasis(Emphasis::new(vec![text_node(value)]));
    writes_and_reads_back(
        &line_of(vec![strong("x "), emphasis(" y")]),
        "**x&#32;***&#32;y*\n",
    );
    writes_and_reads_back(
        &line_of(vec![strong("x"), emphasis("(y)")]),
        "**&#120;***(y)*\n",
    );
    writes_and_reads_back(
        &line_of(vec![
            text_node("a"),
            strong("(x)"),
            emphasis("(y)"),
            text_node("b"),
        ]),
        "&#97;**(x)***(y)*&#98;\n",
    );
}

#[cfg(feature = "binary-format")]
#[test]
fn the_neighbours_of_a_mark_are_written_as_references_from_its_archived_tree_too() {
    let strong = |children| Inline::Strong(Strong::new(children));
    let emphasis = |children| Inline::Emphasis(Emphasis::new(children));
    let link = Link::new(
        LinkForm::Markdown,
        "https://example.com".to_owned(),
        None,
        vec![
            LinkInline::Text(Text::new("a".to_owned())),
            LinkInline::Strong(LinkStrong::new(vec![LinkInline::Text(Text::new(
                "(bold)".to_owned(),
            ))])),
            LinkInline::Text(Text::new("b".to_owned())),
        ],
    );
    let between = |inner| vec![text_node("a"), inner, text_node("b")];
    for (children, expected) in [
        (
            between(strong(vec![text_node("(bold)")])),
            "&#97;**(bold)**&#98;\n",
        ),
        (
            vec![strong(vec![text_node("bold.")]), text_node("b")],
            "**bold.**&#98;\n",
        ),
        (
            between(emphasis(vec![text_node("(it)")])),
            "&#97;*(it)*&#98;\n",
        ),
        (
            between(strong(vec![Inline::Code(Code::new("x".to_owned()))])),
            "&#97;**`x`**&#98;\n",
        ),
        (
            between(strong(vec![text_node(" bold")])),
            "&#97;**&#32;bold**b\n",
        ),
        (
            between(strong(vec![text_node(" bold ")])),
            "&#97;**&#32;bold&#32;**&#98;\n",
        ),
        (
            vec![Inline::Link(link)],
            "[&#97;**(bold)**&#98;](https://example.com)\n",
        ),
        (
            vec![
                strong(vec![text_node("x ")]),
                emphasis(vec![text_node(" y")]),
            ],
            "**x&#32;***&#32;y*\n",
        ),
        (
            vec![
                strong(vec![text_node("x")]),
                emphasis(vec![text_node("(y)")]),
            ],
            "**&#120;***(y)*\n",
        ),
        (
            vec![
                text_node("a"),
                strong(vec![text_node("(x)")]),
                emphasis(vec![text_node("(y)")]),
                text_node("b"),
            ],
            "&#97;**(x)***(y)*&#98;\n",
        ),
    ] {
        let bytes = archive(&RawDocument::Valid(line_of(children)));
        assert_eq!(
            write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
            expected
        );
    }
}

/// Trees holding an emphasis or a strong that writes nothing, each with the
/// text it is written as.
fn empty_spans() -> [(Document, &'static str); 5] {
    let empty_strong = || Inline::Strong(Strong::new(Vec::new()));
    let alone = document(vec![Section::new(
        None,
        vec![
            Block::Paragraph(Paragraph::new(vec![Line::new(vec![empty_strong()])])),
            Block::Paragraph(Paragraph::new(vec![Line::new(vec![text_node("next")])])),
        ],
        Vec::new(),
    )]);
    [
        (
            line_of(vec![text_node("a"), empty_strong(), text_node("b")]),
            "ab\n",
        ),
        (
            line_of(vec![text_node("a "), empty_strong(), text_node(" b")]),
            "a  b\n",
        ),
        (
            line_of(vec![
                text_node("a"),
                Inline::Emphasis(Emphasis::new(vec![empty_strong()])),
                text_node("b"),
            ]),
            "ab\n",
        ),
        (
            line_of(vec![
                text_node("a"),
                Inline::Strong(Strong::new(vec![text_node("")])),
                text_node("b"),
            ]),
            "ab\n",
        ),
        (alone, "next\n"),
    ]
}

#[test]
fn an_emphasis_or_strong_that_writes_nothing_is_written_as_nothing() {
    for (built, expected) in empty_spans() {
        assert_eq!(
            write_valid(&built, &WriteOptions::default()).unwrap(),
            expected
        );
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn an_emphasis_or_strong_that_writes_nothing_is_written_as_nothing_from_its_archived_tree_too() {
    for (built, expected) in empty_spans() {
        let bytes = archive(&RawDocument::Valid(built));
        assert_eq!(
            write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
            expected
        );
    }
}

/// A document of one section holding `blocks`.
fn blocks_of(blocks: Vec<Block>) -> Document {
    document(vec![Section::new(None, blocks, Vec::new())])
}

fn paragraph_of(lines: Vec<Line>) -> Block {
    Block::Paragraph(Paragraph::new(lines))
}

/// Trees holding a paragraph line that writes nothing, each with the text
/// it is written as.
fn empty_lines() -> [(Document, &'static str); 5] {
    let next = || paragraph_of(vec![Line::new(vec![text_node("next")])]);
    [
        (
            blocks_of(vec![paragraph_of(vec![Line::new(Vec::new())]), next()]),
            "next\n",
        ),
        (
            blocks_of(vec![
                paragraph_of(vec![Line::new(vec![text_node("")])]),
                next(),
            ]),
            "next\n",
        ),
        (
            blocks_of(vec![paragraph_of(vec![
                Line::new(vec![text_node("a")]),
                Line::new(Vec::new()),
                Line::new(vec![text_node("b")]),
            ])]),
            "a\nb\n",
        ),
        (
            blocks_of(vec![
                paragraph_of(vec![Line::new(vec![text_node("a")])]),
                paragraph_of(vec![Line::new(Vec::new())]),
                next(),
            ]),
            "a\n\nnext\n",
        ),
        (
            blocks_of(vec![paragraph_of(vec![
                Line::new(Vec::new()),
                Line::new(vec![text_node("[a]: b")]),
            ])]),
            "\\[a]: b\n",
        ),
    ]
}

#[test]
fn a_paragraph_line_that_writes_nothing_is_not_written() {
    for (built, expected) in empty_lines() {
        let text = write_valid(&built, &WriteOptions::default()).unwrap();
        assert_eq!(text, expected);
        assert_eq!(written(&text), text);
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn a_paragraph_line_that_writes_nothing_is_not_written_from_its_archived_tree_too() {
    for (built, expected) in empty_lines() {
        let bytes = archive(&RawDocument::Valid(built));
        assert_eq!(
            write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
            expected
        );
    }
}

#[test]
fn the_lines_around_a_line_that_writes_nothing_read_back_as_one_paragraph() {
    let a = || Line::new(vec![text_node("a")]);
    let b = || Line::new(vec![text_node("b")]);
    let built = blocks_of(vec![paragraph_of(vec![a(), Line::new(Vec::new()), b()])]);
    let text = write_valid(&built, &WriteOptions::default()).unwrap();
    assert_eq!(
        body(&text),
        blocks_of(vec![paragraph_of(vec![a(), b()])]).body
    );
}

#[test]
fn a_strong_over_a_line_break_in_an_underlined_heading_keeps_its_marks() {
    let source = "**a\\\nb**\n===\n";
    assert_eq!(round_trip(source), source);
    #[cfg(feature = "binary-format")]
    {
        let bytes = archive(&parse(source).document);
        assert_eq!(
            write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
            source
        );
    }
}

#[test]
fn a_line_break_in_a_heading_deeper_than_two_is_written_as_a_reference() {
    let line_break = Inline::LineBreak(LineBreak { kind: Kind(None) });
    let third = headed(vec![text_node("a"), line_break, text_node("b")], Vec::new());
    let built = document(vec![headed(
        Vec::new(),
        vec![headed(Vec::new(), vec![third])],
    )]);
    assert_eq!(
        write_valid(&built, &WriteOptions::default()).unwrap(),
        "#\n\n##\n\n### a&#10;b\n"
    );
}

#[test]
fn a_simple_table_and_a_one_line_grid_table_read_back_to_the_same_body() {
    assert_eq!(
        round_trip("a   b\n--- ---\n1   2\n"),
        "| a   | b   |\n| :-- | :-- |\n| 1   | 2   |\n"
    );
    assert_eq!(
        round_trip("+---+---+\n| a | b |\n+===+===+\n| 1 | 2 |\n+---+---+\n"),
        "| a   | b   |\n| --- | --- |\n| 1   | 2   |\n"
    );
}

#[test]
fn toml_args_are_written_as_pairs_separated_by_commas() {
    assert_eq!(
        round_trip("```md title=\"README.md\",showLineNumbers=true\nx\n```\n"),
        "```md title = \"README.md\", showLineNumbers = true\nx\n```\n"
    );
}

#[test]
fn toml_args_with_dotted_keys_that_share_a_prefix_are_written_as_one_table() {
    assert_eq!(
        round_trip("```md a.b=1,a.c=2\nx\n```\n"),
        "```md a = { b = 1, c = 2 }\nx\n```\n"
    );
}

#[test]
fn text_args_with_a_key_and_no_value_are_written_as_found() {
    let source = "```md title=\"README.md\" showLineNumbers\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn a_word_after_the_language_is_written_as_found() {
    let source = "```Markdown title\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn words_after_the_language_are_written_as_found() {
    let source = "```markdown Example of a list\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn text_args_with_a_key_written_twice_are_written_as_found() {
    let source = "```md a=1,a=2\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn text_args_whose_inline_table_ends_before_the_last_brace_are_written_as_found() {
    let source = "```md a=1} # x\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn a_fence_with_no_info_string_is_written_with_none() {
    let source = "```\nx\n```\n";
    assert_eq!(round_trip(source), source);
}

#[test]
fn a_code_block_holding_a_fence_indented_four_columns_is_fenced_longer() {
    assert_eq!(
        round_trip("```\ncode\n    ```\nafter\n"),
        "````\ncode\n    ```\nafter\n````\n"
    );
}

#[test]
fn a_code_block_whose_args_hold_a_backtick_is_fenced_with_tildes() {
    for (source, text) in [
        ("~~~sh a`b\nx\n~~~\n", "~~~sh a`b\nx\n~~~\n"),
        ("~~~sh a`b\n```\n~~~\n", "~~~sh a`b\n```\n~~~\n"),
        ("~~~sh a`b\n~~~ x\n~~~\n", "~~~~sh a`b\n~~~ x\n~~~~\n"),
    ] {
        assert_eq!(round_trip(source), text, "{source:?}");
    }
}

#[test]
fn a_markdown_block_keeps_its_info_string_and_its_claimed_content() {
    let source = "```Markdown title\n# A\n```\n";
    assert_eq!(round_trip(source), source);
    let claimed = match &body(source).sections[0].blocks[0] {
        Block::CodeBlock(code) => code.kind.0.as_deref().cloned(),
        Block::BlockQuote(_)
        | Block::Html(_)
        | Block::List(_)
        | Block::Paragraph(_)
        | Block::Table(_)
        | Block::ThematicBreak(_) => None,
    };
    assert_eq!(claimed, Some(parse("# A\n").document));
}

/// A code block whose language is `language` and whose `Toml` args map
/// `title` to `value`, then `b` to 1.
fn code_with_args(language: &str, value: Value, kind: Kind<Markdown>) -> Block {
    let mapping = IndexMap::from([
        ("title".to_owned(), value),
        ("b".to_owned(), Value::Integer(1)),
    ]);
    Block::CodeBlock(CodeBlock {
        args: Args::Toml(mapping),
        kind,
        language: Some(language.to_owned()),
        value: "x".to_owned(),
    })
}

/// A markdown block of one section with no heading, holding `blocks`.
fn holding(blocks: Vec<Block>) -> MarkdownBlock {
    MarkdownBlock {
        kind: Kind(None),
        sections: vec![Section {
            blocks,
            heading: None,
            kind: Kind(None),
            subsections: Vec::new(),
        }],
    }
}

#[test]
fn toml_args_holding_a_null_fail_the_write_with_an_error_naming_the_key() {
    let null = || code_with_args("sh", Value::Null(()), Kind(None));
    let mut writer = Writer::new();
    if let Block::CodeBlock(code) = null() {
        assert_eq!(writer.info_string(&code), "sh b = 1");
    }
    assert_eq!(writer.error, Some(null_error("title")));
    let in_item = Block::List(List {
        items: vec![Item {
            body: holding(vec![null()]),
            kind: Kind(None),
        }],
        kind: Kind(None),
        loose: false,
        start: None,
    });
    let in_cell = Block::Table(Table {
        alignments: vec![Alignment::Default],
        format: TableFormat::Pipe,
        head: vec![TableRow {
            cells: vec![TableCell {
                body: holding(vec![null()]),
                kind: Kind(None),
            }],
            kind: Kind(None),
        }],
        kind: Kind(None),
        rows: Vec::new(),
    });
    let claimed = Kind(Some(Box::new(RawDocument::Valid(Document {
        body: holding(vec![null()]),
        ..document(Vec::new())
    }))));
    let in_markdown = code_with_args("md", Value::Integer(2), claimed);
    let claimed_null = Kind(Some(Box::new(RawDocument::Valid(document(Vec::new())))));
    let markdown_null = code_with_args("md", Value::Null(()), claimed_null);
    for block in [null(), in_item, in_cell, in_markdown, markdown_null] {
        let built = Document {
            body: holding(vec![block]),
            ..document(Vec::new())
        };
        assert_eq!(
            write_valid(&built, &WriteOptions::default()),
            Err(null_error("title"))
        );
    }
}

#[test]
fn a_broken_frontmatter_is_written_back_as_it_was_read_whatever_syntax_is_picked() {
    for source in [
        "---\n{a: 1, b: [}\n---\n# A\n",
        "+++\na = \"\"\"never closed\n+++\n# A\n",
    ] {
        let document = parse(source).document;
        assert!(matches!(document, RawDocument::Invalid(_)), "{source:?}");
        for options in [WriteOptions::default(), TOML] {
            let text = write(&document, &options).unwrap();
            assert_eq!(text, source);
            assert_eq!(parse(&text).document, document);
        }
    }
}

#[test]
fn a_broken_frontmatter_whose_text_holds_a_line_that_would_close_it_fails_the_write() {
    for (source, text, line) in [
        ("---\na: [\n---\n# A\n", "a: [\n---\nb: 1", "---"),
        ("---\na: [\n---\n# A\n", "a: [\n...  \nb: 1", "..."),
        ("+++\na = [\n+++\n# A\n", "a = [\n+++\nb = 1", "+++"),
    ] {
        let parse_options = ParseOptions::default();
        let mut document = parse_with::<Plain>(source, &parse_options)
            .unwrap()
            .document;
        if let RawDocument::Invalid(invalid) = &mut document {
            invalid.frontmatter.text = text.to_owned();
        }
        let error = WriteError {
            key: String::new(),
            message: format!(
                "the invalid frontmatter's text holds the line `{line}`, which would close it"
            ),
        };
        for options in [WriteOptions::default(), TOML] {
            assert_eq!(write(&document, &options), Err(error.clone()), "{text:?}");
        }
        assert_eq!(
            rebuild_with::<Plain>(&document, &parse_options),
            Err(RebuildError::Write(error)),
            "{text:?}"
        );
    }
}

#[test]
fn a_broken_frontmatter_whose_text_holds_a_delimiter_inside_a_line_is_written_as_it_was_read() {
    for source in [
        "---\n{a: ---x, b: [}\n---\n# A\n",
        "---\n{a: 1, b: [}\n---x\n---\n# A\n",
        "+++\na = \"\"\"never closed\n---\n+++\n# A\n",
    ] {
        let document = parse(source).document;
        assert!(matches!(document, RawDocument::Invalid(_)), "{source:?}");
        for options in [WriteOptions::default(), TOML] {
            let text = write(&document, &options).unwrap();
            assert_eq!(text, source);
            assert_eq!(parse(&text).document, document);
        }
    }
}

fn toml(source: &str) -> String {
    write(&parse(source).document, &TOML).unwrap()
}

fn toml_error(source: &str) -> WriteError {
    write(&parse(source).document, &TOML).unwrap_err()
}

/// The frontmatter mapping `source` reads as.
fn mapping(source: &str) -> Mapping {
    valid(parse(source)).frontmatter.unwrap()
}

/// `mapping` with each time of day turned into the text it is written as
/// in YAML.
fn times_as_text(mapping: &mut Mapping) {
    mapping.values_mut().for_each(time_as_text);
}

fn time_as_text(value: &mut Value) {
    match value {
        Value::Mapping(mapping) => times_as_text(mapping),
        Value::Sequence(items) => items.iter_mut().for_each(time_as_text),
        Value::Time(time) => *value = Value::String(time.to_string()),
        Value::Boolean(_)
        | Value::Date(_)
        | Value::DateTime(_)
        | Value::Float(_)
        | Value::Integer(_)
        | Value::Null(())
        | Value::OffsetDateTime(_)
        | Value::String(_) => {}
    }
}

fn null_error(key: &str) -> WriteError {
    WriteError {
        key: key.to_owned(),
        message: "TOML cannot hold a null".to_owned(),
    }
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// A document whose frontmatter holds `mapping` and whose body is empty.
fn with_frontmatter(mapping: Mapping) -> Document {
    Document {
        frontmatter: Some(mapping),
        ..document(Vec::new())
    }
}

/// A document whose frontmatter maps `x` to `value`.
fn with_frontmatter_value(value: Value) -> Document {
    with_frontmatter(IndexMap::from([("x".to_owned(), value)]))
}

fn written_yaml(document: &Document) -> String {
    write_valid(document, &WriteOptions::default()).unwrap()
}

fn written_toml(document: &Document) -> String {
    write_valid(document, &TOML).unwrap()
}

/// The value `x` reads back as from the YAML and the TOML that `document`
/// is written as, which must agree.
fn read_back_value(document: &Document) -> Value {
    let [yaml, toml_value] = [written_yaml(document), written_toml(document)]
        .map(|text| mapping(&text).swap_remove("x").unwrap());
    assert_eq!(yaml, toml_value);
    yaml
}

#[test]
fn toml_frontmatter_is_written_as_yaml_by_default() {
    assert_eq!(
        written(TOML_SAMPLE),
        "---\ntitle: Notes\ndue: !!timestamp 2026-10-10\nparams:\n  author: Ada\n---\n# Notes\n"
    );
}

#[test]
fn toml_frontmatter_is_written_between_plus_lines_when_picked() {
    assert_eq!(
        toml(TOML_SAMPLE),
        "+++\ntitle = \"Notes\"\ndue = 2026-10-10\n\n[params]\nauthor = \"Ada\"\n+++\n# Notes\n"
    );
}

#[test]
fn keys_keep_the_order_they_were_read_and_a_key_read_twice_is_written_once() {
    let source = "---\nz: 1\nt: A\na: 2\nt: B\n---\n";
    assert_eq!(written(source), "---\nz: 1\nt: [A, B]\na: 2\n---\n");
    assert_eq!(toml(source), "+++\nz = 1\nt = [\"A\", \"B\"]\na = 2\n+++\n");
}

#[test]
fn yaml_is_written_in_block_form_with_text_quoted_only_when_needed() {
    let source = "---
title: 'Notes'
owner:
  name: Ada
  team:
    lead: Grace
tags: [a, b]
people:
  - name: Ada
    role: host
  - Grace
nested: [[a], []]
none: {}
empty: ''
a: \"yes\"
b: \"2026-10-10\"
c: \"12\"
pair: \"a: b\"
comment: \"a #b\"
dash: \"- a\"
lines: \"one\\ntwo\"
nothing: null
due: 2026-10-10
at: 2026-10-10T07:32:00Z
local: 2026-10-10T07:32:00.5
---
";
    let text = written(source);
    assert_eq!(
        text,
        "---
title: Notes
owner:
  name: Ada
  team:
    lead: Grace
tags: [a, b]
people:
  - name: Ada
    role: host
  - Grace
nested: [[a], []]
none: {}
empty: \"\"
a: yes
b: \"2026-10-10\"
c: \"12\"
pair: \"a: b\"
comment: \"a #b\"
dash: \"- a\"
lines: \"one\\ntwo\"
nothing: null
due: !!timestamp 2026-10-10
at: !!timestamp 2026-10-10T07:32:00Z
local: !!timestamp 2026-10-10T07:32:00.500
---
"
    );
    assert_eq!(mapping(&text), mapping(source));
}

#[test]
fn a_list_of_mappings_is_written_in_block_form_inside_a_list_too() {
    assert_eq!(
        written("---\np:\n  - name: Ada\n---\n"),
        "---\np:\n  - name: Ada\n---\n"
    );
    let source = "---\np: [[{name: Ada}, b], c]\n---\n";
    let text = written(source);
    assert_eq!(text, "---\np:\n  - - name: Ada\n    - b\n  - c\n---\n");
    assert_eq!(mapping(&text), mapping(source));
}

#[test]
fn a_time_of_day_is_written_into_yaml_as_quoted_text() {
    assert_eq!(
        written("+++\nat = 07:32:00\n+++\n"),
        "---\nat: \"07:32:00\"\n---\n"
    );
}

#[test]
fn a_time_of_day_written_into_yaml_writes_the_same_quoted_text_again() {
    let text = written("+++\nat = 07:32:00\n+++\nx\n");
    assert_eq!(text, "---\nat: \"07:32:00\"\n---\nx\n");
    assert_eq!(written(&text), text);
    let fraction = "---\nat: \"07:32:00.5\"\n---\nx\n";
    assert_eq!(written(fraction), fraction);
}

#[test]
fn a_non_finite_float_is_text_and_written_back_quoted() {
    let source = "---\nbig: .inf\nup: .Inf\ndown: -.inf\nnothing: .NaN\n---\n";
    let yaml = written(source);
    assert_eq!(
        yaml,
        "---\nbig: \".inf\"\nup: \".Inf\"\ndown: \"-.inf\"\nnothing: \".NaN\"\n---\n"
    );
    assert_eq!(mapping(&yaml), mapping(source));
    let toml_text = toml(source);
    assert_eq!(
        toml_text,
        "+++\nbig = \".inf\"\nup = \".Inf\"\ndown = \"-.inf\"\nnothing = \".NaN\"\n+++\n"
    );
    assert_eq!(mapping(&toml_text), mapping(source));

    let toml_source = "+++\nbig = inf\nsmall = -inf\n+++\n";
    assert_eq!(
        toml(toml_source),
        "+++\nbig = \"inf\"\nsmall = \"-inf\"\n+++\n"
    );
    assert_eq!(mapping(&toml(toml_source)), mapping(toml_source));
    assert_eq!(
        written(toml_source),
        "---\nbig: inf\nsmall: \"-inf\"\n---\n"
    );
    assert_eq!(mapping(&written(toml_source)), mapping(toml_source));
}

#[test]
fn a_hand_built_non_finite_float_is_written_as_the_quoted_text_of_its_literal() {
    for (float, yaml, toml_text) in [
        (f64::NAN, ".nan", "nan"),
        (f64::INFINITY, ".inf", "inf"),
        (f64::NEG_INFINITY, "-.inf", "-inf"),
    ] {
        let document = with_frontmatter_value(Value::Float(float));
        let yaml_written = written_yaml(&document);
        assert!(
            yaml_written.contains(&format!("x: \"{yaml}\"\n")),
            "{yaml_written}"
        );
        let toml_written = written_toml(&document);
        assert!(
            toml_written.contains(&format!("x = \"{toml_text}\"\n")),
            "{toml_written}"
        );
        assert_eq!(mapping(&yaml_written)["x"], text(yaml));
        assert_eq!(mapping(&toml_written)["x"], text(toml_text));
    }
}

#[test]
fn a_hand_built_finite_float_is_written_as_the_shortest_text_that_reads_back_as_it() {
    for (float, text) in [(1.5_f64, "1.5"), (2.0_f64, "2.0")] {
        let document = with_frontmatter_value(Value::Float(float));
        assert!(written_yaml(&document).contains(&format!("x: {text}\n")));
        assert!(written_toml(&document).contains(&format!("x = {text}\n")));
        assert_eq!(read_back_value(&document), Value::Float(float));
    }
}

#[test]
fn text_holding_a_colon_and_a_tab_is_written_into_yaml_quoted_and_reads_back_as_itself() {
    let document = with_frontmatter_value(Value::String("b:\tc".to_owned()));
    assert!(written_yaml(&document).contains("x: \"b:\\tc\"\n"));
    assert_eq!(
        read_back_value(&document),
        Value::String("b:\tc".to_owned())
    );
}

#[test]
fn text_that_reads_as_a_float_with_no_digit_before_its_dot_is_written_into_yaml_quoted() {
    let document = with_frontmatter_value(Value::String(".5".to_owned()));
    assert!(written_yaml(&document).contains("x: \".5\"\n"));
    assert_eq!(read_back_value(&document), Value::String(".5".to_owned()));
}

#[test]
fn text_of_an_integer_too_large_for_64_bits_is_written_into_yaml_quoted_and_reads_back_as_itself() {
    let digits = "99999999999999999999";
    let document = with_frontmatter_value(Value::String(digits.to_owned()));
    assert!(written_yaml(&document).contains(&format!("x: \"{digits}\"\n")));
    assert_eq!(read_back_value(&document), Value::String(digits.to_owned()));
}

#[test]
fn a_float_too_large_for_a_float_read_from_either_format_is_written_as_quoted_text() {
    for source in ["---\nbig: 1e400\n---\n", "+++\nbig = 1e400\n+++\n"] {
        assert_eq!(mapping(source)["big"], Value::String("1e400".to_owned()));
        let yaml_text = written(source);
        assert_eq!(yaml_text, "---\nbig: \"1e400\"\n---\n");
        assert_eq!(mapping(&yaml_text), mapping(source));
        let toml_text = toml(source);
        assert_eq!(toml_text, "+++\nbig = \"1e400\"\n+++\n");
        assert_eq!(mapping(&toml_text), mapping(source));
    }
}

#[test]
fn an_integer_too_large_for_64_bits_read_from_either_format_is_written_as_quoted_text() {
    let yaml = "---\nn: 99999999999999999999\n---\n";
    let toml_source = "+++\nn = 99999999999999999999\n+++\n";
    for source in [yaml, toml_source] {
        let yaml_text = written(source);
        assert_eq!(yaml_text, "---\nn: \"99999999999999999999\"\n---\n");
        assert_eq!(mapping(&yaml_text), mapping(source));
        let toml_text = toml(source);
        assert_eq!(toml_text, "+++\nn = \"99999999999999999999\"\n+++\n");
        assert_eq!(mapping(&toml_text), mapping(source));
    }
}

#[test]
fn toml_quotes_all_text_and_puts_tables_after_the_plain_keys() {
    let source = "---
title: Notes
owner: [Ada, {name: Grace}]
params:
  author: Ada
  extra:
    toc: true
  tags: [a, b]
count: 3
people:
  - name: Ada
  - name: Grace
ratio: 0.5
whole: 2.0
done: false
due: 2026-10-10
at: 2026-10-10T07:32:00-04:00
local: 2026-10-10T07:32:00
none: {}
quote: 'say \"hi\" \\ now'
lines: \"one\\ntwo\"
---
";
    let text = toml(source);
    assert_eq!(
        text,
        "+++
title = \"Notes\"
owner = [\"Ada\", { name = \"Grace\" }]
count = 3
ratio = 0.5
whole = 2.0
done = false
due = 2026-10-10
at = 2026-10-10T07:32:00-04:00
local = 2026-10-10T07:32:00
quote = \"say \\\"hi\\\" \\\\ now\"
lines = \"one\\ntwo\"

[params]
author = \"Ada\"
tags = [\"a\", \"b\"]

[params.extra]
toc = true

[[people]]
name = \"Ada\"

[[people]]
name = \"Grace\"

[none]
+++
"
    );
    assert_eq!(mapping(&text), mapping(source));
    assert_eq!(
        toml("+++\nat = 07:32:00\n+++\n"),
        "+++\nat = 07:32:00\n+++\n"
    );
}

#[test]
fn a_null_cannot_be_written_into_toml() {
    assert_eq!(toml_error("---\na: null\n---\n"), null_error("a"));
    assert_eq!(
        toml_error("---\nowner:\n  name: Ada\n  email: null\n---\n"),
        null_error("owner.email")
    );
    assert_eq!(
        toml_error("---\nowner: [Ada, {email: ~}]\n---\n"),
        null_error("owner.email")
    );
    assert_eq!(
        toml_error("---\np:\n  - name:\n---\n"),
        null_error("p.name")
    );
    assert_eq!(toml_error("---\na.b: null\n---\n"), null_error("\"a.b\""));
}

#[test]
fn every_fixture_frontmatter_reads_back_to_the_same_mapping_in_yaml_and_in_toml() {
    let mut with_frontmatter_count = 0_usize;
    for (name, source) in FIXTURES {
        if valid(parse(source)).frontmatter.is_none() {
            continue;
        }
        with_frontmatter_count += 1;
        let mut expected = mapping(source);
        match name {
            "meeting_notes" => assert_eq!(toml_error(source), null_error("recording")),
            "recipe" => assert_eq!(toml_error(source), null_error("source")),
            _ => assert_eq!(mapping(&toml(source)), expected, "{name}"),
        }
        times_as_text(&mut expected);
        assert_eq!(mapping(&written(source)), expected, "{name}");
    }
    assert_eq!(with_frontmatter_count, 15);
}

#[test]
fn a_body_line_after_the_frontmatter_is_not_escaped() {
    let source = "---\na: 1\n---\n\\+++\n";
    assert_eq!(round_trip(source), "---\na: 1\n---\n+++\n");
    let text = toml(source);
    assert_eq!(text, "+++\na = 1\n+++\n+++\n");
    assert_eq!(mapping(&text), mapping(source));
    assert_eq!(body(&text), body(source));
}

#[test]
fn a_byte_order_mark_at_the_start_of_a_file_is_not_written() {
    let yaml = "\u{feff}---\na: 1\n---\n# A\n";
    let yaml_text = round_trip(yaml);
    assert_eq!(yaml_text, "---\na: 1\n---\n# A\n");
    assert_eq!(mapping(&yaml_text), mapping(yaml));
    assert_eq!(round_trip("\u{feff}# A\n\ntext\n"), "# A\n\ntext\n");
    let plus = "\u{feff}+++\na = 1\n+++\nx\n";
    assert_eq!(round_trip(plus), "---\na: 1\n---\nx\n");
    let toml_text = toml(plus);
    assert_eq!(toml_text, "+++\na = 1\n+++\nx\n");
    assert_eq!(toml(&toml_text), toml_text);
    assert_eq!(mapping(&toml_text), mapping(plus));
    assert_eq!(body(&toml_text), body(plus));
}

#[test]
fn a_byte_order_mark_that_would_start_the_file_is_written_as_a_reference() {
    let text = round_trip("\u{feff}\u{feff}# A\n");
    assert_eq!(text, "&#65279;# A\n");
    assert_eq!(round_trip(&text), text);
}

#[test]
fn a_byte_order_mark_after_the_start_of_the_file_is_written_as_it_is() {
    assert_eq!(round_trip("a\u{feff}b\n"), "a\u{feff}b\n");
    assert_eq!(
        round_trip("---\na: 1\n---\n\u{feff}x\n"),
        "---\na: 1\n---\n\u{feff}x\n"
    );
}

#[test]
fn an_alias_is_written_as_a_copy_of_its_anchors_value_and_no_anchor_is_written() {
    assert_eq!(
        written("---\nauthor: &ada Ada\nreviewer: *ada\n---\n"),
        "---\nauthor: Ada\nreviewer: Ada\n---\n"
    );
    assert_eq!(
        written("---\ndefaults: &d\n  draft: false\n  lang: en\npost: *d\n---\n# Notes\n"),
        "---\ndefaults:\n  draft: false\n  lang: en\npost:\n  draft: false\n  lang: en\n---\n# Notes\n"
    );
}

#[test]
fn a_write_leaves_every_dropped_entry_out() {
    let document =
        parse("---\ntitle: Notes\ntags: [a, b\ndate: 2026-10-17\n---\n# Body\n").document;
    assert_eq!(
        write(&document, &WriteOptions::default()).unwrap(),
        "---\ntitle: Notes\ndate: !!timestamp 2026-10-17\n---\n# Body\n"
    );
    let toml = parse("+++\ntitle = Notes\ndate = 2026-10-17\n+++\n# Body\n").document;
    assert_eq!(
        write(&toml, &TOML).unwrap(),
        "+++\ndate = 2026-10-17\n+++\n# Body\n"
    );
}

#[test]
fn the_owners_sample_frontmatter_is_written_with_its_quotes_dropped() {
    let text = written(OWNERS_SAMPLE);
    assert!(
        text.starts_with("---\ntitle: Notes\ntags: [a, b]\n---\n# Notes\n"),
        "{text}"
    );
}

#[test]
fn text_reads_back_as_the_same_text_whether_it_is_quoted_or_not() {
    let texts = [
        "",
        " a",
        "a ",
        "a: b",
        "a:",
        "a:b",
        "a #b",
        "a#b",
        "#a",
        "- a",
        "-a",
        "? a",
        ":a",
        "[a]",
        "{a}",
        "a,b",
        "a]b",
        "'a'",
        "\"a\"",
        "!a",
        "&a",
        "*a",
        "|a",
        ">a",
        "%a",
        "@a",
        "`a`",
        "one\ntwo",
        "tab\there",
        "null",
        "~",
        "true",
        "False",
        "12",
        "-5",
        "1.5",
        "1e5",
        ".inf",
        "0x1F",
        "2026-10-10",
        "2026-02-30",
        "2026-10-10T07:32:00Z",
        "yes",
        "07:32:00",
        "a b",
        "back\\slash",
        "say \"hi\"",
        "https://x.org/a#b",
        "\u{1}",
        "\u{7f}",
        "\u{85}",
        "\u{e9}t\u{e9}",
        "---",
        "...",
        "+++",
    ];
    let mut texts_by_key: Mapping = texts
        .iter()
        .map(|value| ((*value).to_owned(), text(value)))
        .collect();
    let list = Value::Sequence(texts.iter().map(|value| text(value)).collect());
    texts_by_key.insert("list".to_owned(), list);
    let built = with_frontmatter(texts_by_key);
    let expected = built.frontmatter.clone().unwrap();
    for frontmatter in [FrontmatterFormat::Toml, FrontmatterFormat::Yaml] {
        let written_text = write_valid(&built, &WriteOptions { frontmatter }).unwrap();
        assert_eq!(mapping(&written_text), expected, "{written_text}");
    }
}

#[cfg(feature = "binary-format")]
/// The rkyv bytes of `document`.
fn archive(document: &RawDocument) -> AlignedVec {
    rkyv::to_bytes::<Error>(With::<RawDocument, RawDocumentMirror>::cast(document)).unwrap()
}

#[cfg(feature = "binary-format")]
/// The archived tree `bytes` hold, checked.
fn archived(bytes: &[u8]) -> &ArchivedRawDocument {
    rkyv::access::<ArchivedRawDocument, Error>(bytes).unwrap()
}

#[cfg(feature = "binary-format")]
/// Checks that the archived tree of `document` writes, with YAML and with
/// TOML options, what `write` gives for `document` with every kind empty.
fn writes_as_write_does(mut document: RawDocument, name: &str) {
    let bytes = archive(&document);
    let tree = archived(&bytes);
    unclaim(&mut document);
    for options in [WriteOptions::default(), TOML] {
        assert_eq!(
            write_archived(tree, &options),
            write(&document, &options),
            "{name}: {options:?}"
        );
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn the_sample_is_written_from_its_archived_tree() {
    let parsed = parse("---\nstatus: open\n---\n# Task\n\n* item\n");
    let bytes = archive(&parsed.document);
    assert_eq!(
        write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
        "---\nstatus: open\n---\n# Task\n\n- item\n"
    );
}

#[cfg(feature = "binary-format")]
#[test]
fn each_fixture_is_written_from_its_archived_tree_as_write_writes_it() {
    for (name, source) in FIXTURES {
        writes_as_write_does(parse(source).document, name);
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn each_construct_is_written_from_its_archived_tree_as_write_writes_it() {
    for source in CONSTRUCTS.into_iter().chain([OWNERS_SAMPLE, TOML_SAMPLE]) {
        writes_as_write_does(parse(source).document, source);
    }
    let line_break = Inline::LineBreak(LineBreak { kind: Kind(None) });
    let third = headed(vec![text_node("a"), line_break, text_node("b")], Vec::new());
    let deep_heading = document(vec![headed(
        Vec::new(),
        vec![headed(Vec::new(), vec![third])],
    )]);
    let wide = Block::Table(Table {
        alignments: vec![Alignment::Left, Alignment::Right, Alignment::Center],
        format: TableFormat::Pipe,
        head: vec![TableRow {
            cells: vec![TableCell {
                body: holding(vec![Block::Paragraph(Paragraph {
                    kind: Kind(None),
                    lines: vec![Line {
                        children: vec![text_node("a")],
                        kind: Kind(None),
                    }],
                })]),
                kind: Kind(None),
            }],
            kind: Kind(None),
        }],
        kind: Kind(None),
        rows: Vec::new(),
    });
    let null_args = Document {
        body: holding(vec![code_with_args("sh", Value::Null(()), Kind(None))]),
        ..document(Vec::new())
    };
    for (name, built) in [
        ("sections 22 levels deep", document(vec![levels(1, 22)])),
        ("a line break in a heading deeper than two", deep_heading),
        ("toml args holding a null", null_args),
        (
            "more alignments than cells",
            Document {
                body: holding(vec![wide]),
                ..document(Vec::new())
            },
        ),
    ] {
        writes_as_write_does(RawDocument::Valid(built), name);
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn each_value_is_written_from_its_archived_tree_as_write_writes_it() {
    let date = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let leap_second = NaiveTime::from_hms_nano_opt(23, 59, 59, 1_500_000_000).unwrap();
    let local = date.and_time(NaiveTime::from_hms_nano_opt(9, 0, 0, 123_456_789).unwrap());
    let at = FixedOffset::west_opt(4 * 3600)
        .unwrap()
        .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap())
        .single()
        .unwrap();
    let inner: Mapping = IndexMap::from([("a b".to_owned(), Value::Integer(1))]);
    let values: Mapping = IndexMap::from([
        ("boolean".to_owned(), Value::Boolean(true)),
        ("date".to_owned(), Value::Date(date)),
        ("date-time".to_owned(), Value::DateTime(local)),
        ("float".to_owned(), Value::Float(0.5)),
        (
            "built non-finite".to_owned(),
            Value::Float(f64::NEG_INFINITY),
        ),
        ("integer".to_owned(), Value::Integer(-7)),
        ("empty".to_owned(), Value::Mapping(Mapping::new())),
        ("mapping".to_owned(), Value::Mapping(inner.clone())),
        ("offset-date-time".to_owned(), Value::OffsetDateTime(at)),
        (
            "sequence".to_owned(),
            Value::Sequence(vec![text("a: b"), Value::Time(leap_second)]),
        ),
        (
            "tables".to_owned(),
            Value::Sequence(vec![Value::Mapping(inner.clone()), Value::Mapping(inner)]),
        ),
        (
            "mixed".to_owned(),
            Value::Sequence(vec![Value::Integer(1), Value::Mapping(Mapping::new())]),
        ),
        ("string".to_owned(), text("07:32:00")),
        ("time".to_owned(), Value::Time(leap_second)),
    ]);
    let mut with_null = values.clone();
    with_null.insert("null".to_owned(), Value::Null(()));
    for (name, mapping) in [("each value", values), ("each value and a null", with_null)] {
        writes_as_write_does(RawDocument::Valid(with_frontmatter(mapping)), name);
    }
}

#[cfg(feature = "binary-format")]
#[test]
fn the_tree_at_the_caps_is_written_from_its_archived_tree_as_write_writes_it() {
    on_two_megabytes(|| {
        let mut source = String::from("---\n");
        for level in 0..VALUE_CAP {
            source.push_str(&"  ".repeat(level));
            source.push_str("a:\n");
        }
        source.push_str(&"  ".repeat(VALUE_CAP));
        source.push_str("b: 1\n---\n");
        source.push_str(&"- ".repeat(BLOCK_CAP - 1));
        source.push_str("a\n");
        let stars = "*".repeat(2 * INLINE_CAP);
        for deep in [
            source,
            format!("{stars}a{stars}\n"),
            nested_grid_tables(BLOCK_CAP),
        ] {
            writes_as_write_does(parse(&deep).document, "the tree at the caps");
        }
    });
}

#[cfg(feature = "binary-format")]
#[test]
fn a_null_in_the_frontmatter_fails_the_archived_toml_write_with_the_error_write_gives() {
    let document = parse("---\nowner:\n  name: Ada\n  email: null\n---\n").document;
    let bytes = archive(&document);
    assert_eq!(
        write_archived(archived(&bytes), &TOML),
        Err(null_error("owner.email"))
    );
    assert_eq!(write(&document, &TOML), Err(null_error("owner.email")));
}

#[cfg(feature = "binary-format")]
#[test]
fn a_markdown_block_is_written_from_its_archived_tree_as_its_value() {
    let source = "```markdown\n* a\n```\n";
    let document = parse(source).document;
    let bytes = archive(&document);
    assert_eq!(
        write_archived(archived(&bytes), &WriteOptions::default()).unwrap(),
        source
    );
    assert_eq!(
        write(&document, &WriteOptions::default()).unwrap(),
        "```markdown\n- a\n```\n"
    );
}

#[cfg(feature = "binary-format")]
#[test]
fn an_invalid_document_is_written_from_its_archived_tree_as_write_writes_it() {
    for source in [
        "---\n{a: 1, b: [}\n---\n# A\n",
        "+++\na = \"\"\"never closed\n+++\n# A\n",
    ] {
        let document = parse(source).document;
        assert!(matches!(document, RawDocument::Invalid(_)), "{source:?}");
        writes_as_write_does(document, source);
    }
    let mut closing = parse("---\na: [\n---\n# A\n").document;
    if let RawDocument::Invalid(invalid) = &mut closing {
        invalid.frontmatter.text = "a: [\n---\nb: 1".to_owned();
    }
    let error = WriteError {
        key: String::new(),
        message: "the invalid frontmatter's text holds the line `---`, which would close it"
            .to_owned(),
    };
    assert_eq!(write(&closing, &WriteOptions::default()), Err(error));
    writes_as_write_does(closing, "a text holding a line that would close it");
}

#[cfg(feature = "binary-format")]
#[test]
fn an_archived_date_out_of_chronos_range_fails_the_write_with_an_error_naming_its_key() {
    let days = 0x0012_3456_i32;
    let date = NaiveDate::from_num_days_from_ce_opt(days).unwrap();
    let due: Mapping = IndexMap::from([("due".to_owned(), Value::Date(date))]);
    let built = with_frontmatter(IndexMap::from([("owner".to_owned(), Value::Mapping(due))]));
    let mut bytes = archive(&RawDocument::Valid(built));
    let [found, past_range] =
        [days, i32::MAX].map(|count| rkyv::to_bytes::<Error>(&count).unwrap());
    let at = bytes
        .windows(found.len())
        .position(|window| window == found.as_slice())
        .unwrap();
    bytes[at..at + found.len()].copy_from_slice(&past_range);
    let error = WriteError {
        key: "owner.due".to_owned(),
        message: "the archived date is out of chrono's range".to_owned(),
    };
    for options in [WriteOptions::default(), TOML] {
        assert_eq!(
            write_archived(archived(&bytes), &options),
            Err(error.clone())
        );
    }
}
