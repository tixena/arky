//! What a consumer of arky adds to the tree: its own node types, which it
//! gives to the nodes it claims.
//!
//! An [`Extension`] has a node type, a `claim` function and a `write` function. arky builds the
//! whole tree with a byte span on every node, then offers each node to the extensions, children
//! before parents, as a [`Construct`]. A node an extension claims goes into the `kind` field of
//! the plain node, and [`write`](crate::writer::write) writes it back through the extension's
//! `write`, which receives the plain node as a [`Written`] and a [`Writer`].
//!
//! [`parse`](crate::parser::parse) reads with [`Markdown`] alone, arky's own extension, which
//! reads a `markdown` or `md` code block as a whole document.
//! [`parse_with`](crate::parser::parse_with) reads with `(Markdown, E)`, which [`With`] names: a
//! pair of extensions is one extension, and the first that claims a construct gets it, so a node
//! `E` claims sits in `kind` as [`Either::Second`]. [`Plain`] claims nothing. `markdown_schema`
//! is the module tixschema writes beside `Markdown`, as the [`ast`] page describes.
//!
//! This extension claims a code block whose language is `shout` as its text in capitals, and
//! writes the block back as it was:
//!
//! ```rust
//! use arky::parser::ast::{Block, RawDocument};
//! use arky::parser::error::ParseError;
//! use arky::parser::extension::{Construct, Context, Either, Extension, Written};
//! use arky::parser::{ParseOptions, parse_with};
//! use arky::writer::{WriteOptions, Writer, write};
//!
//! #[derive(Debug, Clone, PartialEq)]
//! struct Shout;
//!
//! impl Extension for Shout {
//!     type Node<T>
//!         = String
//!     where
//!         T: Extension;
//!
//!     fn claim<T>(
//!         construct: Construct<'_, T>,
//!         _context: &Context<'_>,
//!     ) -> Option<Result<String, ParseError>>
//!     where
//!         T: Extension,
//!     {
//!         let Construct::CodeBlock(block) = construct else {
//!             return None;
//!         };
//!         (block.language.as_deref() == Some("shout")).then(|| Ok(block.value.to_uppercase()))
//!     }
//!
//!     fn write<T>(_node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
//!     where
//!         T: Extension,
//!     {
//!         writer.construct(plain);
//!     }
//! }
//!
//! let source = "```shout\nhello\n```\n";
//! let parsed = parse_with::<Shout>(source, &ParseOptions::default())?;
//! let RawDocument::Valid(document) = &parsed.document else {
//!     panic!("a file with no frontmatter is valid");
//! };
//! let Some(Block::CodeBlock(block)) = document.body.sections[0].blocks.first() else {
//!     panic!("the file is one code block");
//! };
//! assert_eq!(block.value, "hello");
//! assert_eq!(block.kind.0.as_deref(), Some(&Either::Second("HELLO".to_owned())));
//! assert_eq!(write(&parsed.document, &WriteOptions::default())?, source);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! An extension that reads a claimed node's text points its errors at the source. This one
//! claims a `task` block as the TOML it holds. [`Context::lines_of`] gives the parts of the
//! block's lines past any list item's indentation,
//! [`read_toml_lines`](crate::parser::read_toml_lines) reads them, and [`ParseError::new`]
//! points an error at the block's [`span`](located::CodeBlock::span). Where the claim fails, a
//! lenient parse keeps the plain code block, and a strict parse fails with the error.
//! [`Context::markdown`] and [`Context::document`] read a claimed range as markdown, as
//! [`Markdown`] reads a `markdown` block.
//!
//! ```rust
//! use arky::parser::ast::{Block, Mapping, RawDocument, Value};
//! use arky::parser::error::ParseError;
//! use arky::parser::extension::{Construct, Context, Either, Extension, Written};
//! use arky::parser::{ParseOptions, parse_with, read_toml_lines};
//! use arky::writer::Writer;
//!
//! #[derive(Debug, Clone, PartialEq)]
//! struct Task;
//!
//! impl Extension for Task {
//!     type Node<T>
//!         = Mapping
//!     where
//!         T: Extension;
//!
//!     fn claim<T>(
//!         construct: Construct<'_, T>,
//!         context: &Context<'_>,
//!     ) -> Option<Result<Mapping, ParseError>>
//!     where
//!         T: Extension,
//!     {
//!         let Construct::CodeBlock(block) = construct else {
//!             return None;
//!         };
//!         if block.language.as_deref() != Some("task") {
//!             return None;
//!         }
//!         let lines = context.lines_of(block.content.clone());
//!         let task = match read_toml_lines(context.source, &lines) {
//!             Ok(task) => task,
//!             Err(error) => return Some(Err(error)),
//!         };
//!         if task.contains_key("title") {
//!             Some(Ok(task))
//!         } else {
//!             Some(Err(ParseError::new("a task needs a title", block.span.clone())))
//!         }
//!     }
//!
//!     fn write<T>(_node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
//!     where
//!         T: Extension,
//!     {
//!         writer.construct(plain);
//!     }
//! }
//!
//! let untitled = "```task\ndue = 2026-11-02\n```\n";
//! let lenient = parse_with::<Task>(untitled, &ParseOptions::default())?;
//! let RawDocument::Valid(document) = &lenient.document else {
//!     panic!("a file with no frontmatter is valid");
//! };
//! let Some(Block::CodeBlock(block)) = document.body.sections[0].blocks.first() else {
//!     panic!("the file is one code block");
//! };
//! assert!(block.kind.0.is_none());
//!
//! let strict = parse_with::<Task>(untitled, &ParseOptions::strict());
//! let error = strict.expect_err("a strict parse fails on the extension's error");
//! assert_eq!(error.to_string(), "a task needs a title (at byte 0)");
//!
//! let titled = "```task\ntitle = \"Buy flour\"\n```\n";
//! let parsed = parse_with::<Task>(titled, &ParseOptions::default())?;
//! let RawDocument::Valid(document) = &parsed.document else {
//!     panic!("a file with no frontmatter is valid");
//! };
//! let Some(Block::CodeBlock(block)) = document.body.sections[0].blocks.first() else {
//!     panic!("the file is one code block");
//! };
//! let Some(Either::Second(task)) = block.kind.0.as_deref() else {
//!     panic!("the task extension claims the block");
//! };
//! assert_eq!(task.get("title"), Some(&Value::String("Buy flour".to_owned())));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use core::convert::Infallible;
use core::fmt::Debug;
use core::iter;
use core::ops::Range;

use serde::{Deserialize, Serialize};
use tixschema::model_schema;

use crate::parser::ast::{self, Document, InvalidDocument, Kind, MarkdownBlock, RawDocument};
use crate::parser::error::ParseError;
use crate::parser::line::{Base, line_from};
use crate::parser::{block, claim, frontmatter, located};
use crate::writer::Writer;

/// A consumer's node types. arky builds every node first, children before
/// parents, and then offers it to the extension.
///
/// The [`extension`](crate::parser::extension) module page shows one in full.
pub trait Extension: Clone + Debug + PartialEq + Sized {
    /// The node an extension builds. `T` is the full set of extensions, so a
    /// node can hold arky nodes typed with it.
    type Node<T>: Clone + Debug + PartialEq
    where
        T: Extension;

    /// Claims `construct`, a node of the located tree, spans included, or leaves it.
    ///
    /// `None` leaves the construct to arky, and `Some(Ok(..))` claims it. `Some(Err(..))` falls
    /// back to the plain node, or fails a strict parse. No construct that 40 or more list items,
    /// markdown blocks and table cells hold is offered: it stays the node arky built.
    fn claim<T>(
        construct: Construct<'_, T>,
        context: &Context<'_>,
    ) -> Option<Result<Self::Node<T>, ParseError>>
    where
        T: Extension;

    /// Writes a claimed node back. `plain` is the node arky built under it, and
    /// [`Writer::construct`] writes that plain node in arky's form.
    fn write<T>(node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension;
}

/// A located node arky offers to the extensions while it parses, typed
/// with the full set `T`.
///
/// Each variant holds the node of the same name in [`located`], whose spans count from the start
/// of the source.
#[derive(Clone)]
#[non_exhaustive]
pub enum Construct<'node, T>
where
    T: Extension,
{
    /// A block quote.
    BlockQuote(&'node located::BlockQuote<T>),
    /// A code span.
    Code(&'node located::Code<T>),
    /// A fenced or indented code block.
    CodeBlock(&'node located::CodeBlock<T>),
    /// The whole file.
    Document(&'node located::Document<T>),
    /// An emphasis, also one inside a link's text.
    Emphasis(&'node located::Emphasis<T>),
    /// A heading line.
    Heading(&'node located::Heading<T>),
    /// Raw HTML, a block or inline.
    Html(&'node located::Html<T>),
    /// An image embed.
    Image(&'node located::Image<T>),
    /// A list item.
    Item(&'node located::Item<T>),
    /// A line of a paragraph.
    Line(&'node located::Line<T>),
    /// A hard line break.
    LineBreak(&'node located::LineBreak<T>),
    /// A link.
    Link(&'node located::Link<T>),
    /// A list.
    List(&'node located::List<T>),
    /// A markdown block: the body, an item's, a cell's or a `markdown` code block's.
    MarkdownBlock(&'node located::MarkdownBlock<T>),
    /// A paragraph.
    Paragraph(&'node located::Paragraph<T>),
    /// A section.
    Section(&'node located::Section<T>),
    /// A strong emphasis, also one inside a link's text.
    Strong(&'node located::Strong<T>),
    /// A table.
    Table(&'node located::Table<T>),
    /// A table cell.
    TableCell(&'node located::TableCell<T>),
    /// A table row.
    TableRow(&'node located::TableRow<T>),
    /// Plain text.
    Text(&'node located::Text<T>),
    /// A thematic break.
    ThematicBreak(&'node located::ThematicBreak<T>),
}

/// A model node a claimed node is written over: the plain node under it.
/// An emphasis or a strong claimed inside a link's text comes back as
/// `LinkEmphasis` or `LinkStrong`.
///
/// Each variant holds the node of the same name in [`ast`].
#[derive(Clone)]
#[non_exhaustive]
pub enum Written<'node, T>
where
    T: Extension,
{
    /// A block quote.
    BlockQuote(&'node ast::BlockQuote<T>),
    /// A code span.
    Code(&'node ast::Code<T>),
    /// A code block.
    CodeBlock(&'node ast::CodeBlock<T>),
    /// The whole file.
    Document(&'node Document<T>),
    /// An emphasis.
    Emphasis(&'node ast::Emphasis<T>),
    /// A heading line.
    Heading(&'node ast::Heading<T>),
    /// Raw HTML, a block or inline.
    Html(&'node ast::Html<T>),
    /// An image embed.
    Image(&'node ast::Image<T>),
    /// A list item.
    Item(&'node ast::Item<T>),
    /// A line of a paragraph.
    Line(&'node ast::Line<T>),
    /// A hard line break.
    LineBreak(&'node ast::LineBreak<T>),
    /// A link.
    Link(&'node ast::Link<T>),
    /// An emphasis inside a link's text.
    LinkEmphasis(&'node ast::LinkEmphasis<T>),
    /// A strong emphasis inside a link's text.
    LinkStrong(&'node ast::LinkStrong<T>),
    /// A list.
    List(&'node ast::List<T>),
    /// A markdown block.
    MarkdownBlock(&'node MarkdownBlock<T>),
    /// A paragraph.
    Paragraph(&'node ast::Paragraph<T>),
    /// A section.
    Section(&'node ast::Section<T>),
    /// A strong emphasis.
    Strong(&'node ast::Strong<T>),
    /// A table.
    Table(&'node ast::Table<T>),
    /// A table cell.
    TableCell(&'node ast::TableCell<T>),
    /// A table row.
    TableRow(&'node ast::TableRow<T>),
    /// Plain text.
    Text(&'node ast::Text<T>),
    /// A thematic break.
    ThematicBreak(&'node ast::ThematicBreak<T>),
}

/// What an extension reads a construct with: where the construct starts on
/// its line, how many containers hold it, the lines of the grid cell it lies
/// in, the source text, and whether the parse is strict.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Context<'text> {
    /// The column the offered construct's first line starts at, counted
    /// from the start of its cell's line in a grid cell, in the frame of the
    /// markdown block that holds it.
    pub column: usize,
    /// How many list items, markdown blocks and table cells hold the offered
    /// construct. No construct that 40 or more of them hold is offered.
    pub depth: usize,
    /// The column each markdown block around the construct reads its lines past. Only arky
    /// reads it.
    pub frame: Frame<'text>,
    /// The lines of the grid cell the offered construct lies in, as ranges
    /// of the source, or `None` outside grid cells.
    pub lines: Option<&'text [Range<usize>]>,
    /// The whole text being parsed, which every span indexes.
    pub source: &'text str,
    /// Whether the parse is strict, so that an error fails it.
    pub strict: bool,
}

impl Context<'_> {
    /// Reads `range` as a whole document, its lines read as `markdown` reads
    /// them: a frontmatter on its first lines, then its body, claimed by
    /// arky's markdown extension alone when no entry of the frontmatter reads.
    ///
    /// # Errors
    ///
    /// In a strict parse, returns the error of the first entry the
    /// frontmatter drops, of a frontmatter no entry of reads, or of an
    /// extension that claims a construct inside `range` and cannot read it.
    pub fn document<T>(&self, range: Range<usize>) -> Result<RawDocument<T>, ParseError>
    where
        T: Extension,
    {
        let lines = self.framed(&range);
        let mut starts = Vec::with_capacity(lines.len());
        let mut text = String::new();
        for line in &lines {
            if !starts.is_empty() {
                text.push('\n');
            }
            starts.push(text.len());
            text.extend(iter::repeat_n(' ', line.columns));
            text.push_str(&self.source[line.text.clone()]);
        }
        let place = |at: usize| {
            let index = starts
                .partition_point(|&start| start <= at)
                .saturating_sub(1);
            lines
                .get(index)
                .zip(starts.get(index))
                .map_or(at, |(line, &start)| {
                    line.text.start + (at - start).saturating_sub(line.columns)
                })
        };
        let (read, body_start) =
            frontmatter::parse::<ParseError>(&text, 0, self.strict).map_err(|error| {
                ParseError {
                    span: place(error.span.start)..place(error.span.end),
                    ..error
                }
            })?;
        let body = match starts.iter().position(|&start| start == body_start) {
            Some(0) => range,
            Some(index) => lines[index].start..range.end,
            None => range.end..range.end,
        };
        if let Some(Err(broken)) = read {
            return Ok(RawDocument::Invalid(InvalidDocument {
                body: self.markdown::<Markdown>(body)?,
                frontmatter: broken.into_model(),
            }));
        }
        Ok(RawDocument::Valid(Document {
            body: self.markdown::<T>(body)?,
            frontmatter: read.and_then(Result::ok).map(|kept| kept.mapping),
            kind: Kind(None),
        }))
    }

    /// The lines of `range` as `markdown` reads them: on each of the grid
    /// cell's lines, or else from `range` on, each read past the frame and
    /// `column`.
    fn framed(&self, range: &Range<usize>) -> Vec<Framed> {
        let base = Base {
            columns: self.column,
            frame: self.frame.0,
        };
        let Some(cell) = self.lines else {
            let source = &self.source[..range.end];
            let mut lines = Vec::new();
            let mut start = range.start;
            while start < source.len() {
                let line = line_from(source, start, base);
                lines.push(Framed {
                    columns: line.column,
                    start,
                    text: line.start..line.end,
                });
                start = line.next;
            }
            return lines;
        };
        // A cell's parts are read as `lex_lines` reads them: as the lines of
        // one text, each starting its own columns.
        let parts = within(cell, range);
        let mut joined = String::new();
        let mut starts = Vec::with_capacity(parts.len());
        for part in &parts {
            if !starts.is_empty() {
                joined.push('\n');
            }
            starts.push(joined.len());
            joined.push_str(&self.source[part.clone()]);
        }
        parts
            .iter()
            .zip(starts)
            .map(|(part, start)| {
                let line = line_from(&joined, start, base);
                Framed {
                    columns: line.column,
                    start: part.start,
                    text: part.start + (line.start - start)..part.start + (line.end - start),
                }
            })
            .collect()
    }

    /// The parts of `range` on the construct's own lines: on each of the
    /// grid cell's lines, or else `range` split at each `\n`, which it leaves
    /// out, each line read past the frame and `column`, as `markdown` reads it.
    #[must_use]
    pub fn lines_of(&self, range: Range<usize>) -> Vec<Range<usize>> {
        if let Some(lines) = self.lines {
            return within(lines, &range);
        }
        let base = Base {
            columns: self.column,
            frame: self.frame.0,
        };
        let mut line_start = self.source[..range.start]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        let mut start = range.start;
        self.source[range]
            .split('\n')
            .map(|line| {
                let end = start + line.len();
                let indented = line_from(self.source, line_start, base).start.min(end);
                let part = start.max(indented)..end;
                start = end + 1;
                line_start = start;
                part
            })
            .collect()
    }

    /// The located tree `markdown` reads and claims, before it moves it into
    /// the model.
    pub(crate) fn located_markdown<T>(
        &self,
        range: Range<usize>,
    ) -> Result<located::MarkdownBlock<T>, ParseError>
    where
        T: Extension,
    {
        let depth = self.depth + 1;
        let base = Base {
            columns: self.column,
            frame: self.frame.0,
        };
        let (mut read, _, cells) = match self.lines {
            Some(lines) => {
                block::parse_lines(self.source, &within(lines, &range), base, depth, range)
            }
            None => block::parse_from(&self.source[..range.end], range.start, base, depth),
        };
        let frame = [self.frame.0, &[self.column]].concat();
        let inside = Context {
            depth,
            frame: Frame(&frame),
            ..*self
        };
        claim::markdown_block(&mut read, &inside, &cells)?;
        Ok(read)
    }

    /// Reads `range` of the source as a markdown block, with the extensions
    /// `T`, each line read past the frame of the markdown block that holds
    /// the construct, then with at most `column` columns of its leading white
    /// space left out and its tab stops counted from there.
    /// In a grid cell, only the parts of `range` on the cell's lines are
    /// read, one after another.
    /// The block is one container deeper than the construct. While it is
    /// claimed, its spans count from the start of the source.
    ///
    /// # Errors
    ///
    /// In a strict parse, returns the error of an extension that claims a
    /// construct inside `range` and cannot read it.
    pub fn markdown<T>(&self, range: Range<usize>) -> Result<MarkdownBlock<T>, ParseError>
    where
        T: Extension,
    {
        self.located_markdown(range)
            .map(located::MarkdownBlock::into_model)
    }
}

/// The column each markdown block that holds a construct reads its lines
/// past, outermost first, from the start of the line or, in a grid cell, of
/// its cell's line. Only arky reads it.
///
/// It has no public constructor: an extension finds it in [`Context::frame`].
#[derive(Debug, Clone, Copy)]
pub struct Frame<'text>(&'text [usize]);

impl<'text> Frame<'text> {
    /// The frame of a construct that no markdown block holds.
    pub(crate) const NONE: Self = Self(&[]);

    pub(crate) const fn columns(self) -> &'text [usize] {
        self.0
    }
}

/// A line as `Context::markdown` reads it: where it starts, how many columns
/// of white space it keeps before its text, and its text.
struct Framed {
    columns: usize,
    start: usize,
    text: Range<usize>,
}

/// An extension that claims nothing: [`parse_with`](crate::parser::parse_with) with `Plain`
/// parses as [`parse`](crate::parser::parse) does, its tree typed with `(Markdown, Plain)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Plain;

impl Extension for Plain {
    type Node<T>
        = Infallible
    where
        T: Extension;

    fn claim<T>(
        _construct: Construct<'_, T>,
        _context: &Context<'_>,
    ) -> Option<Result<Infallible, ParseError>>
    where
        T: Extension,
    {
        None
    }

    /// Never called: `Plain` claims nothing, so it has no node to write.
    fn write<T>(_node: &Infallible, _plain: Written<'_, T>, _writer: &mut Writer)
    where
        T: Extension,
    {
    }
}

/// arky's own extension: a fenced block whose language is the word
/// `markdown` or `md`, in any case, holds a whole document, its frontmatter
/// included.
///
/// Its node is the `RawDocument` the block's lines read as, and it writes that document back
/// inside a fence with the block's info string.
#[model_schema()]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Markdown;

impl Extension for Markdown {
    type Node<T>
        = RawDocument<T>
    where
        T: Extension;

    fn claim<T>(
        construct: Construct<'_, T>,
        context: &Context<'_>,
    ) -> Option<Result<Self::Node<T>, ParseError>>
    where
        T: Extension,
    {
        let Construct::CodeBlock(block) = construct else {
            return None;
        };
        let language = block.language.as_deref()?;
        (language.eq_ignore_ascii_case("markdown") || language.eq_ignore_ascii_case("md"))
            .then(|| context.document::<T>(block.content.clone()))
    }

    /// Fenced with the info string of the code block it was read from.
    fn write<T>(node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension,
    {
        let info = if let Written::CodeBlock(block) = plain {
            writer.info_string(block)
        } else {
            "markdown".to_owned()
        };
        writer.fenced(&info, |inner| inner.raw_document(node));
    }
}

/// The node of a pair of extensions, `(A, B)`, which is itself an extension. The first one that
/// claims a construct gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Either<A, B> {
    /// A node of the first extension.
    First(A),
    /// A node of the second extension.
    Second(B),
}

impl<A, B> Extension for (A, B)
where
    A: Extension,
    B: Extension,
{
    type Node<T>
        = Either<A::Node<T>, B::Node<T>>
    where
        T: Extension;

    fn claim<T>(
        construct: Construct<'_, T>,
        context: &Context<'_>,
    ) -> Option<Result<Self::Node<T>, ParseError>>
    where
        T: Extension,
    {
        A::claim(construct.clone(), context)
            .map(|result| result.map(Either::First))
            .or_else(|| B::claim(construct, context).map(|result| result.map(Either::Second)))
    }

    fn write<T>(node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension,
    {
        match node {
            Either::First(first) => A::write(first, plain, writer),
            Either::Second(second) => B::write(second, plain, writer),
        }
    }
}

/// The extension set arky parses with: its own markdown extension first,
/// then the consumer's.
pub type With<E> = (Markdown, E);

/// The part of `range` on each of `lines` that it reaches.
fn within(lines: &[Range<usize>], range: &Range<usize>) -> Vec<Range<usize>> {
    lines
        .iter()
        .filter(|line| line.start <= range.end && range.start <= line.end)
        .map(|line| line.start.max(range.start)..line.end.min(range.end))
        .collect()
}
