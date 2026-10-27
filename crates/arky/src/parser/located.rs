//! The tree the parser builds, every node with the byte range it covers.
//!
//! An extension receives it as the [`Construct`](crate::parser::extension::Construct) that
//! `Extension::claim` is offered, and reads spans from it to point its errors at the source.
//! Its frontmatter values are as the TOML and YAML readers build them. At the
//! end of a parse, `into_model` moves each node into the model, spans left out.

use core::mem;
use core::ops::Range;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;

use crate::parser::ast::{
    self, Alignment, Args, Continues, FrontmatterFormat, Kind, LinkForm, TableFormat,
};
use crate::parser::error::ParseError;
use crate::parser::extension::{Extension, Markdown};
use crate::parser::inline::children_mut;

/// A whole markdown file.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Document<E = Markdown>
where
    E: Extension,
{
    /// The markdown after the frontmatter, or the whole file when it has none.
    pub body: MarkdownBlock<E>,
    /// The frontmatter: `None` when the file has none, an error when no entry of it reads.
    pub frontmatter: Option<Result<Frontmatter, BrokenFrontmatter>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

/// A frontmatter that arky could read no entry of: its text as written,
/// kept so that a write gives it back, and the reader's error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BrokenFrontmatter {
    /// The error of the first entry the reader refused or dropped.
    pub error: ParseError,
    /// The syntax the frontmatter is written in.
    pub format: FrontmatterFormat,
    /// The byte range of the frontmatter, its delimiter lines included.
    pub span: Range<usize>,
    /// The text between the delimiter lines, without the line ending of its last line.
    pub text: String,
}

impl BrokenFrontmatter {
    /// The model's frontmatter: the reader's message, the syntax and the text.
    #[must_use]
    pub fn into_model(self) -> ast::InvalidFrontmatter {
        ast::InvalidFrontmatter {
            error: self.error.message,
            format: self.format,
            text: self.text,
        }
    }
}

/// The metadata block at the top of a file: TOML between `+++` lines or
/// YAML between `---` lines.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Frontmatter {
    /// Each entry and key that could not be read and was left out, in file
    /// order.
    pub dropped: Vec<ParseError>,
    /// The syntax the frontmatter is written in.
    pub format: FrontmatterFormat,
    /// The entries that read.
    pub mapping: ast::Mapping,
    /// The byte range of the frontmatter, its delimiter lines included.
    pub span: Range<usize>,
}

/// A stretch of markdown read by the same rules: the body, a list item's
/// content, a table cell's content, a markdown code block's content.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MarkdownBlock<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// Its sections, in order.
    pub sections: Vec<Section<E>>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> MarkdownBlock<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::MarkdownBlock<E> {
        ast::MarkdownBlock {
            kind: self.kind,
            sections: self.sections.into_iter().map(Section::into_model).collect(),
        }
    }
}

/// A node that stands on its own lines.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Block<E = Markdown>
where
    E: Extension,
{
    /// Lines marked with `>`, kept as raw text.
    BlockQuote(BlockQuote<E>),
    /// A fenced or indented code block.
    CodeBlock(CodeBlock<E>),
    /// Raw HTML on lines of its own.
    Html(Html<E>),
    /// A bullet or ordered list.
    List(List<E>),
    /// Lines of text.
    Paragraph(Paragraph<E>),
    /// A pipe, grid or simple table.
    Table(Table<E>),
    /// A line such as `---`.
    ThematicBreak(ThematicBreak<E>),
}

impl<E> Block<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Block<E> {
        match self {
            Self::BlockQuote(block_quote) => ast::Block::BlockQuote(block_quote.into_model()),
            Self::CodeBlock(code_block) => ast::Block::CodeBlock(code_block.into_model()),
            Self::Html(html) => ast::Block::Html(html.into_model()),
            Self::List(list) => ast::Block::List(list.into_model()),
            Self::Paragraph(paragraph) => ast::Block::Paragraph(paragraph.into_model()),
            Self::Table(table) => ast::Block::Table(table.into_model()),
            Self::ThematicBreak(thematic_break) => {
                ast::Block::ThematicBreak(thematic_break.into_model())
            }
        }
    }

    /// The byte range the block covers.
    #[must_use]
    pub const fn span(&self) -> &Range<usize> {
        match self {
            Self::BlockQuote(block_quote) => &block_quote.span,
            Self::CodeBlock(code_block) => &code_block.span,
            Self::Html(html) => &html.span,
            Self::List(list) => &list.span,
            Self::Paragraph(paragraph) => &paragraph.span,
            Self::Table(table) => &table.span,
            Self::ThematicBreak(thematic_break) => &thematic_break.span,
        }
    }
}

/// Quoted text, without its `>` marks. Nothing inside it is read.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockQuote<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The quoted lines, each without its `>` mark.
    pub value: String,
}

impl<E> BlockQuote<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::BlockQuote<E> {
        ast::BlockQuote {
            kind: self.kind,
            value: self.value,
        }
    }
}

/// Raw HTML, as written: a block, or an inline node such as `<span>`. A
/// block's `continues` is always `Continues::Neither`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Html<E = Markdown>
where
    E: Extension,
{
    /// Whether an inline piece goes on across a line end.
    pub continues: Continues,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The HTML as written.
    pub value: String,
}

impl<E> Html<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Html<E> {
        ast::Html {
            continues: self.continues,
            kind: self.kind,
            value: self.value,
        }
    }
}

/// A line of three or more `-`, `*` or `_`, such as `---`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ThematicBreak<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> ThematicBreak<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::ThematicBreak<E> {
        ast::ThematicBreak { kind: self.kind }
    }
}

/// A heading, the blocks under it, then its subsections. Its level is its
/// depth in the `MarkdownBlock`.
///
/// `heading` is `None` when no heading line was written, and a heading line
/// with no text is a heading with no children.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Section<E = Markdown>
where
    E: Extension,
{
    /// The blocks between the heading and the first subsection.
    pub blocks: Vec<Block<E>>,
    /// The heading line, or `None` when no heading line was written.
    pub heading: Option<Heading<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The sections one level deeper, in order.
    pub subsections: Vec<Self>,
}

impl<E> Section<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Section<E> {
        ast::Section {
            blocks: self.blocks.into_iter().map(Block::into_model).collect(),
            heading: self.heading.map(Heading::into_model),
            kind: self.kind,
            subsections: self.subsections.into_iter().map(Self::into_model).collect(),
        }
    }
}

/// A heading line, such as `# Bread`. An empty heading has no children
/// and an empty span at the start of its section.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Heading<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes of the heading's text.
    pub children: Vec<Inline<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> Heading<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Heading<E> {
        ast::Heading {
            children: inlines(self.children),
            kind: self.kind,
        }
    }
}

/// Lines of text, ended by a blank line or by a block that starts on the next line, such as a
/// list item, a heading or a fence.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Paragraph<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// Its lines, in order.
    pub lines: Vec<Line<E>>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> Paragraph<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Paragraph<E> {
        ast::Paragraph {
            kind: self.kind,
            lines: self.lines.into_iter().map(Line::into_model).collect(),
        }
    }
}

/// One line of a paragraph and the inline nodes written on it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Line<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes written on the line.
    pub children: Vec<Inline<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> Line<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Line<E> {
        ast::Line {
            children: inlines(self.children),
            kind: self.kind,
        }
    }
}

/// A bullet or ordered list.
///
/// Its items have the same bullet character, or the same delimiter after
/// their number.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct List<E = Markdown>
where
    E: Extension,
{
    /// Its items, in order.
    pub items: Vec<Item<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// Whether a blank line separates two items, or two blocks of one item.
    pub loose: bool,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// An ordered list's first number, or `None` for a bullet list.
    pub start: Option<u64>,
}

impl<E> List<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::List<E> {
        ast::List {
            items: self.items.into_iter().map(Item::into_model).collect(),
            kind: self.kind,
            loose: self.loose,
            start: self.start,
        }
    }
}

/// One entry of a list.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Item<E = Markdown>
where
    E: Extension,
{
    /// The markdown the item holds.
    pub body: MarkdownBlock<E>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> Item<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Item<E> {
        ast::Item {
            body: self.body.into_model(),
            kind: self.kind,
        }
    }
}

/// Lines between two fences of backticks or of tildes, or indented four
/// columns past their parent's.
///
/// The info string, the text after the opening fence, is split into `language`, its first word,
/// and `args`, the rest.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CodeBlock<E = Markdown>
where
    E: Extension,
{
    /// The rest of the info string, after the language.
    pub args: Args,
    /// The byte range of the block's lines in the source, its fences left out. An extension
    /// reads it with [`Context::markdown`](crate::parser::extension::Context::markdown) or
    /// [`Context::document`](crate::parser::extension::Context::document).
    pub content: Range<usize>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The first word of the info string, or `None` when there is no info string.
    pub language: Option<String>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The lines as the block holds them, without the fences or the indentation.
    pub value: String,
}

impl<E> CodeBlock<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::CodeBlock<E> {
        ast::CodeBlock {
            args: self.args,
            kind: self.kind,
            language: self.language,
            value: self.value,
        }
    }
}

/// A table read from pipe, grid or simple table syntax. `head` holds the
/// header rows, `rows` the rest.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Table<E = Markdown>
where
    E: Extension,
{
    /// How each column's cells are aligned, one per column.
    pub alignments: Vec<Alignment>,
    /// The syntax the table was read from.
    pub format: TableFormat,
    /// The header rows.
    pub head: Vec<TableRow<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The rows under the header.
    pub rows: Vec<TableRow<E>>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> Table<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Table<E> {
        ast::Table {
            alignments: self.alignments,
            format: self.format,
            head: self.head.into_iter().map(TableRow::into_model).collect(),
            kind: self.kind,
            rows: self.rows.into_iter().map(TableRow::into_model).collect(),
        }
    }
}

/// One row of a table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TableRow<E = Markdown>
where
    E: Extension,
{
    /// Its cells, one per column.
    pub cells: Vec<TableCell<E>>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> TableRow<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::TableRow<E> {
        ast::TableRow {
            cells: self.cells.into_iter().map(TableCell::into_model).collect(),
            kind: self.kind,
        }
    }
}

/// One cell of a table row.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TableCell<E = Markdown>
where
    E: Extension,
{
    /// The markdown the cell holds.
    pub body: MarkdownBlock<E>,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

impl<E> TableCell<E>
where
    E: Extension,
{
    /// The model's node, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::TableCell<E> {
        ast::TableCell {
            body: self.body.into_model(),
            kind: self.kind,
        }
    }
}

/// A node inside a line of text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Inline<E = Markdown>
where
    E: Extension,
{
    /// Text between backticks.
    Code(Code<E>),
    /// Text between `*` or `_`.
    Emphasis(Emphasis<E>),
    /// Inline HTML, such as `<span>`.
    Html(Html<E>),
    /// An embed, such as `![alt](target)`.
    Image(Image<E>),
    /// A hard line break.
    LineBreak(LineBreak<E>),
    /// A link, such as `[text](target)`.
    Link(Link<E>),
    /// Text between `**` or `__`.
    Strong(Strong<E>),
    /// Plain text.
    Text(Text<E>),
}

impl<E> Inline<E>
where
    E: Extension,
{
    /// The byte range the node covers.
    #[must_use]
    pub const fn span(&self) -> &Range<usize> {
        match self {
            Self::Code(code) => &code.span,
            Self::Emphasis(emphasis) => &emphasis.span,
            Self::Html(html) => &html.span,
            Self::Image(image) => &image.span,
            Self::LineBreak(line_break) => &line_break.span,
            Self::Link(link) => &link.span,
            Self::Strong(strong) => &strong.span,
            Self::Text(text) => &text.span,
        }
    }

    /// The model's node, with `children`, already converted, as its own.
    fn with_children(self, children: Vec<ast::Inline<E>>) -> ast::Inline<E> {
        match self {
            Self::Code(code) => ast::Inline::Code(ast::Code {
                continues: code.continues,
                kind: code.kind,
                value: code.value,
            }),
            Self::Emphasis(emphasis) => ast::Inline::Emphasis(ast::Emphasis {
                children,
                continues: emphasis.continues,
                kind: emphasis.kind,
            }),
            Self::Html(html) => ast::Inline::Html(html.into_model()),
            Self::Image(image) => ast::Inline::Image(ast::Image {
                alt: image.alt,
                continues: image.continues,
                form: image.form,
                kind: image.kind,
                target: image.target,
                title: image.title,
            }),
            Self::LineBreak(line_break) => ast::Inline::LineBreak(ast::LineBreak {
                kind: line_break.kind,
            }),
            Self::Link(link) => ast::Inline::Link(ast::Link {
                children: link_text(children),
                continues: link.continues,
                form: link.form,
                kind: link.kind,
                target: link.target,
                title: link.title,
            }),
            Self::Strong(strong) => ast::Inline::Strong(ast::Strong {
                children,
                continues: strong.continues,
                kind: strong.kind,
            }),
            Self::Text(text) => ast::Inline::Text(ast::Text {
                kind: text.kind,
                value: text.value,
            }),
        }
    }
}

/// Plain text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Text<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The text, its escapes and character references resolved.
    pub value: String,
}

/// Text between `*` or `_`, such as `*a*`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Emphasis<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes between the marks.
    pub children: Vec<Inline<E>>,
    /// Whether this piece goes on across a line end.
    pub continues: Continues,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

/// Text between `**` or `__`, such as `**b**`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Strong<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes between the marks.
    pub children: Vec<Inline<E>>,
    /// Whether this piece goes on across a line end.
    pub continues: Continues,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

/// A hard line break: the two or more spaces, or the backslash, that end a
/// line followed by another line of its paragraph.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LineBreak<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
}

/// A link: `[text](target "title")`, a reference link, an autolink,
/// `[[target]]` or `[[target|text]]`.
///
/// `children` are the text shown, and are empty for `[[target]]`. A
/// reference link takes its target and title from its label's definition.
/// `target` and `title` hold their escapes and references resolved.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Link<E = Markdown>
where
    E: Extension,
{
    /// The text shown, empty for `[[target]]`.
    pub children: Vec<Inline<E>>,
    /// Whether this piece goes on across a line end.
    pub continues: Continues,
    /// How the link is written: `[text](target)` or `[[target|text]]`.
    pub form: LinkForm,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// Where the link goes.
    pub target: String,
    /// The title written after the target, if any.
    pub title: Option<String>,
}

/// An embed: `![alt](target "title")`, `![[target]]` or `![[target|alt]]`.
///
/// `alt` is empty when none is written. `target` and `title` hold their
/// escapes and references resolved.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Image<E = Markdown>
where
    E: Extension,
{
    /// The text in brackets, or the wiki alias after `|`.
    pub alt: String,
    /// Whether this piece goes on across a line end.
    pub continues: Continues,
    /// How the embed is written: `![alt](target)` or `![[target|alt]]`.
    pub form: LinkForm,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The file or address the embed names.
    pub target: String,
    /// The title written after the target, if any.
    pub title: Option<String>,
}

/// Text between backticks, such as `` `cargo test` ``.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Code<E = Markdown>
where
    E: Extension,
{
    /// Whether this piece goes on across a line end.
    pub continues: Continues,
    /// The node an extension gave this one, or none.
    pub kind: Kind<E>,
    /// The byte range the node covers in the source.
    pub span: Range<usize>,
    /// The text between the backticks.
    pub value: String,
}

/// Keys and their values, in the order they were written. A key appears once.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Mapping {
    /// Each key and its pair, in the order the keys were first written.
    pub pairs: IndexMap<String, Pair>,
    /// The byte range the mapping covers in the source.
    pub span: Range<usize>,
}

impl Mapping {
    /// The model's mapping: the same keys in the same order, no spans.
    #[must_use]
    pub fn into_model(self) -> ast::Mapping {
        self.pairs
            .into_iter()
            .map(|(key, pair)| (key, pair.value.into_model()))
            .collect()
    }
}

/// The value under one key of a mapping. Its span runs from the key to the
/// end of the value, at the key's first occurrence. `repeats` holds the span
/// of each later occurrence of the same key, in file order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Pair {
    /// The span of each later occurrence of the key, in file order.
    pub repeats: Vec<Range<usize>>,
    /// The byte range from the key's first occurrence to the end of its value.
    pub span: Range<usize>,
    /// The value under the key.
    pub value: Value,
}

/// What a key maps to, or what a sequence holds.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A table or a mapping.
    Mapping(Mapping),
    /// A single value.
    Scalar(Scalar),
    /// A list.
    Sequence(Sequence),
}

impl Value {
    /// The model's value, its spans left out.
    #[must_use]
    pub fn into_model(self) -> ast::Value {
        match self {
            Self::Mapping(mapping) => ast::Value::Mapping(mapping.into_model()),
            Self::Scalar(scalar) => scalar.value.into_model(),
            Self::Sequence(sequence) => {
                ast::Value::Sequence(sequence.items.into_iter().map(Self::into_model).collect())
            }
        }
    }

    /// The byte range the value covers.
    #[must_use]
    pub const fn span(&self) -> &Range<usize> {
        match self {
            Self::Mapping(mapping) => &mapping.span,
            Self::Scalar(scalar) => &scalar.span,
            Self::Sequence(sequence) => &sequence.span,
        }
    }
}

/// An ordered list of values, such as `["parser", "design"]`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Sequence {
    /// Its values, in order.
    pub items: Vec<Value>,
    /// The byte range the list covers in the source.
    pub span: Range<usize>,
}

/// A single value, such as `2` or `"task"`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Scalar {
    /// The byte range the value covers in the source.
    pub span: Range<usize>,
    /// The value it reads as.
    pub value: ScalarValue,
}

/// The typed content of a scalar.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScalarValue {
    /// `true` or `false`.
    Boolean(bool),
    /// A date, such as `2026-10-17`.
    Date(NaiveDate),
    /// A date and time with no offset.
    DateTime(NaiveDateTime),
    /// A finite float.
    Float(f64),
    /// A whole number.
    Integer(i64),
    /// YAML only: TOML has no null.
    Null,
    /// A date and time with its offset from UTC.
    OffsetDateTime(DateTime<FixedOffset>),
    /// Text.
    String(String),
    /// A time of day, such as `07:32:00`.
    Time(NaiveTime),
}

impl ScalarValue {
    /// The model's value.
    #[must_use]
    pub fn into_model(self) -> ast::Value {
        match self {
            Self::Boolean(boolean) => ast::Value::Boolean(boolean),
            Self::Date(date) => ast::Value::Date(date),
            Self::DateTime(date_time) => ast::Value::DateTime(date_time),
            Self::Float(float) => ast::Value::Float(float),
            Self::Integer(integer) => ast::Value::Integer(integer),
            Self::Null => ast::Value::Null(()),
            Self::OffsetDateTime(date_time) => ast::Value::OffsetDateTime(date_time),
            Self::String(text) => ast::Value::String(text),
            Self::Time(time) => ast::Value::Time(time),
        }
    }
}

/// An emphasis or a strong of a link's text, waiting on its children.
enum Waiting<E>
where
    E: Extension,
{
    Emphasis(Continues, Kind<E>),
    Strong(Continues, Kind<E>),
}

/// The model's nodes for `nodes`, children before their parent. A node
/// waits on a stack, its children taken out of it, while they are
/// converted, so nesting takes no call depth.
fn inlines<E>(nodes: Vec<Inline<E>>) -> Vec<ast::Inline<E>>
where
    E: Extension,
{
    // Each waiting node, with its siblings converted before it and those
    // still to convert after it.
    let mut waiting = Vec::new();
    let mut converted = Vec::new();
    let mut rest = nodes.into_iter();
    loop {
        if let Some(mut node) = rest.next() {
            if let Some(children) = children_mut(&mut node).map(mem::take) {
                let before = mem::take(&mut converted);
                let after = mem::replace(&mut rest, children.into_iter());
                waiting.push((node, before, after));
            } else {
                converted.push(node.with_children(Vec::new()));
            }
        } else if let Some((node, before, after)) = waiting.pop() {
            let children = mem::replace(&mut converted, before);
            converted.push(node.with_children(children));
            rest = after;
        } else {
            return converted;
        }
    }
}

/// `nodes`, a link's converted children, as link text. A link among them,
/// which the parser does not build, is replaced by its own children. A node
/// waits on a stack while its children are converted, so nesting takes no
/// call depth.
fn link_text<E>(nodes: Vec<ast::Inline<E>>) -> Vec<ast::LinkInline<E>>
where
    E: Extension,
{
    // Each waiting node, with its siblings converted before it and those
    // still to convert after it.
    let mut waiting = Vec::new();
    let mut converted = Vec::new();
    let mut rest = nodes.into_iter();
    loop {
        if let Some(node) = rest.next() {
            let opened = match node {
                ast::Inline::Code(code) => {
                    converted.push(ast::LinkInline::Code(code));
                    None
                }
                ast::Inline::Emphasis(emphasis) => Some((
                    Waiting::Emphasis(emphasis.continues, emphasis.kind),
                    emphasis.children,
                )),
                ast::Inline::Html(html) => {
                    converted.push(ast::LinkInline::Html(html));
                    None
                }
                ast::Inline::Image(image) => {
                    converted.push(ast::LinkInline::Image(image));
                    None
                }
                ast::Inline::LineBreak(line_break) => {
                    converted.push(ast::LinkInline::LineBreak(line_break));
                    None
                }
                ast::Inline::Link(link) => {
                    converted.extend(link.children);
                    None
                }
                ast::Inline::Strong(strong) => Some((
                    Waiting::Strong(strong.continues, strong.kind),
                    strong.children,
                )),
                ast::Inline::Text(text) => {
                    converted.push(ast::LinkInline::Text(text));
                    None
                }
            };
            if let Some((parent, children)) = opened {
                let before = mem::take(&mut converted);
                let after = mem::replace(&mut rest, children.into_iter());
                waiting.push((parent, before, after));
            }
        } else if let Some((parent, before, after)) = waiting.pop() {
            let children = mem::replace(&mut converted, before);
            converted.push(match parent {
                Waiting::Emphasis(continues, kind) => {
                    ast::LinkInline::Emphasis(ast::LinkEmphasis {
                        children,
                        continues,
                        kind,
                    })
                }
                Waiting::Strong(continues, kind) => ast::LinkInline::Strong(ast::LinkStrong {
                    children,
                    continues,
                    kind,
                }),
            });
            rest = after;
        } else {
            return converted;
        }
    }
}
