//! What a consumer of arky adds to the tree: its own node types, which it
//! gives to the nodes it claims.

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
pub trait Extension: Clone + Debug + PartialEq + Sized {
    /// The node an extension builds. `T` is the full set of extensions, so a
    /// node can hold arky nodes typed with it.
    type Node<T>: Clone + Debug + PartialEq
    where
        T: Extension;

    /// `None` leaves the construct to arky. `Some(Err(..))` falls back to
    /// the plain node, or fails a strict parse. No construct that
    /// `BLOCK_CAP` or more list items, markdown blocks and table cells hold
    /// is offered: it stays the node arky built.
    fn claim<T>(
        construct: Construct<'_, T>,
        context: &Context<'_>,
    ) -> Option<Result<Self::Node<T>, ParseError>>
    where
        T: Extension;

    /// Writes a claimed node back. `plain` is the node arky built under it.
    fn write<T>(node: &Self::Node<T>, plain: Written<'_, T>, writer: &mut Writer)
    where
        T: Extension;
}

/// A located node arky offers to the extensions while it parses, typed
/// with the full set `T`.
#[derive(Clone)]
#[non_exhaustive]
pub enum Construct<'node, T>
where
    T: Extension,
{
    BlockQuote(&'node located::BlockQuote<T>),
    Code(&'node located::Code<T>),
    CodeBlock(&'node located::CodeBlock<T>),
    Document(&'node located::Document<T>),
    Emphasis(&'node located::Emphasis<T>),
    Heading(&'node located::Heading<T>),
    Html(&'node located::Html<T>),
    Image(&'node located::Image<T>),
    Item(&'node located::Item<T>),
    Line(&'node located::Line<T>),
    LineBreak(&'node located::LineBreak<T>),
    Link(&'node located::Link<T>),
    List(&'node located::List<T>),
    MarkdownBlock(&'node located::MarkdownBlock<T>),
    Paragraph(&'node located::Paragraph<T>),
    Section(&'node located::Section<T>),
    Strong(&'node located::Strong<T>),
    Table(&'node located::Table<T>),
    TableCell(&'node located::TableCell<T>),
    TableRow(&'node located::TableRow<T>),
    Text(&'node located::Text<T>),
    ThematicBreak(&'node located::ThematicBreak<T>),
}

/// A model node a claimed node is written over: the plain node under it.
/// An emphasis or a strong claimed inside a link's text comes back as
/// `LinkEmphasis` or `LinkStrong`.
#[derive(Clone)]
#[non_exhaustive]
pub enum Written<'node, T>
where
    T: Extension,
{
    BlockQuote(&'node ast::BlockQuote<T>),
    Code(&'node ast::Code<T>),
    CodeBlock(&'node ast::CodeBlock<T>),
    Document(&'node Document<T>),
    Emphasis(&'node ast::Emphasis<T>),
    Heading(&'node ast::Heading<T>),
    Html(&'node ast::Html<T>),
    Image(&'node ast::Image<T>),
    Item(&'node ast::Item<T>),
    Line(&'node ast::Line<T>),
    LineBreak(&'node ast::LineBreak<T>),
    Link(&'node ast::Link<T>),
    LinkEmphasis(&'node ast::LinkEmphasis<T>),
    LinkStrong(&'node ast::LinkStrong<T>),
    List(&'node ast::List<T>),
    MarkdownBlock(&'node MarkdownBlock<T>),
    Paragraph(&'node ast::Paragraph<T>),
    Section(&'node ast::Section<T>),
    Strong(&'node ast::Strong<T>),
    Table(&'node ast::Table<T>),
    TableCell(&'node ast::TableCell<T>),
    TableRow(&'node ast::TableRow<T>),
    Text(&'node ast::Text<T>),
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
    /// construct. No construct that `BLOCK_CAP` or more of them hold is
    /// offered.
    pub depth: usize,
    pub frame: Frame<'text>,
    /// The lines of the grid cell the offered construct lies in, as ranges
    /// of the source, or `None` outside grid cells.
    pub lines: Option<&'text [Range<usize>]>,
    pub source: &'text str,
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

/// An extension that claims nothing.
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

/// Two extensions together. The first one that claims a construct gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Either<A, B> {
    First(A),
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
