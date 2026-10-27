use arky::parser::ast::{
    Alignment, Args, Block, CodeBlock, Document, Heading, Inline, Item, Kind, Line, Link, LinkForm,
    LinkInline, LinkStrong, List, Mapping, MarkdownBlock, Paragraph, RawDocument, Section, Strong,
    Table, TableCell, TableFormat, TableRow, Text, ThematicBreak,
};
use arky::parser::error::ParseError;
use arky::parser::extension::{Construct, Context, Either, Extension, Markdown, With, Written};
use arky::parser::{ParseOptions, parse, parse_with};
use arky::writer::{WriteOptions, Writer, write};

const SOURCE: &str = "# Results\n\nRevenue grew **12%**, see [the dashboard](https://example.com/q3).\n\n- North\n- South\n\n| Region | Revenue |\n| ------ | ------: |\n| North  | 1,200   |\n";

const SHOUT: &str = "```shout\nhello\n```\n";

/// Claims a code block whose language is `shout` as its text in capitals.
#[derive(Debug, Clone, PartialEq)]
struct Shout;

impl Extension for Shout {
    type Node<T>
        = String
    where
        T: Extension;

    fn claim<T>(
        construct: Construct<'_, T>,
        _context: &Context<'_>,
    ) -> Option<Result<String, ParseError>>
    where
        T: Extension,
    {
        let Construct::CodeBlock(block) = construct else {
            return None;
        };
        (block.language.as_deref() == Some("shout")).then(|| Ok(block.value.to_uppercase()))
    }

    fn write<T>(_node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension,
    {
        writer.construct(plain);
    }
}

fn text(value: &str) -> Inline<Markdown> {
    Inline::Text(Text::new(value.to_owned()))
}

fn link_text(value: &str) -> LinkInline<Markdown> {
    LinkInline::Text(Text::new(value.to_owned()))
}

fn paragraph_of(value: &str) -> Block<Markdown> {
    Block::Paragraph(Paragraph::new(vec![Line::new(vec![text(value)])]))
}

fn block_of(value: &str) -> MarkdownBlock<Markdown> {
    MarkdownBlock::new(vec![Section::new(
        None,
        vec![paragraph_of(value)],
        Vec::new(),
    )])
}

fn document_of(sections: Vec<Section<Markdown>>) -> RawDocument<Markdown> {
    RawDocument::Valid(Document::new(MarkdownBlock::new(sections), None))
}

fn built() -> RawDocument<Markdown> {
    let line = Line::new(vec![
        text("Revenue grew "),
        Inline::Strong(Strong::new(vec![text("12%")])),
        text(", see "),
        Inline::Link(Link::new(
            LinkForm::Markdown,
            "https://example.com/q3".to_owned(),
            None,
            vec![link_text("the dashboard")],
        )),
        text("."),
    ]);
    let list = List::new(
        vec![Item::new(block_of("North")), Item::new(block_of("South"))],
        false,
        None,
    );
    let table = Table::new(
        TableFormat::Pipe,
        vec![Alignment::Default, Alignment::Right],
        vec![TableRow::new(vec![
            TableCell::new(block_of("Region")),
            TableCell::new(block_of("Revenue")),
        ])],
        vec![TableRow::new(vec![
            TableCell::new(block_of("North")),
            TableCell::new(block_of("1,200")),
        ])],
    );
    let section = Section::new(
        Some(Heading::new(vec![text("Results")])),
        vec![
            Block::Paragraph(Paragraph::new(vec![line])),
            Block::List(list),
            Block::Table(table),
        ],
        Vec::new(),
    );
    document_of(vec![section])
}

fn shouted() -> RawDocument<With<Shout>> {
    let mut block = CodeBlock::new(
        Some("shout".to_owned()),
        Args::Toml(Mapping::new()),
        "hello".to_owned(),
    );
    block.kind = Kind::new(Either::Second("HELLO".to_owned()));
    let section = Section::new(None, vec![Block::CodeBlock(block)], Vec::new());
    RawDocument::Valid(Document::new(MarkdownBlock::new(vec![section]), None))
}

#[test]
fn a_document_built_from_another_crate_equals_the_one_its_markdown_reads_as() {
    assert_eq!(parse(SOURCE).document, built());
}

#[test]
fn a_document_built_from_another_crate_writes_its_markdown() {
    assert_eq!(write(&built(), &WriteOptions::default()).unwrap(), SOURCE);
}

#[test]
fn an_extension_node_built_in_another_crate_equals_the_one_its_extension_reads() {
    assert_eq!(
        parse_with::<Shout>(SHOUT, &ParseOptions::default())
            .unwrap()
            .document,
        shouted()
    );
}

#[test]
fn a_document_with_an_extension_node_built_in_another_crate_writes_its_markdown() {
    assert_eq!(write(&shouted(), &WriteOptions::default()).unwrap(), SHOUT);
}

#[test]
fn a_built_section_takes_blocks_added_removed_and_replaced() {
    let mut section = Section::new(
        Some(Heading::new(vec![text("Results")])),
        vec![
            paragraph_of("Old."),
            Block::ThematicBreak(ThematicBreak::default()),
        ],
        Vec::new(),
    );
    section.blocks[0] = paragraph_of("New.");
    section.blocks.remove(1);
    section.blocks.push(Block::List(List::new(
        vec![Item::new(block_of("North"))],
        false,
        None,
    )));
    assert_eq!(
        parse("# Results\n\nNew.\n\n- North\n").document,
        document_of(vec![section])
    );
}

#[test]
fn a_built_link_takes_its_text_replaced() {
    let mut link = Link::new(
        LinkForm::Markdown,
        "https://example.com/q3".to_owned(),
        None,
        vec![link_text("old")],
    );
    link.children[0] = LinkInline::Strong(LinkStrong::new(vec![link_text("new")]));
    let paragraph = Block::Paragraph(Paragraph::new(vec![Line::new(vec![Inline::Link(link)])]));
    let section = Section::new(None, vec![paragraph], Vec::new());
    assert_eq!(
        parse("[**new**](https://example.com/q3)\n").document,
        document_of(vec![section])
    );
}
