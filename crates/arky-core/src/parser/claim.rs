//! The claim pass: every node of a built tree offered to the extensions,
//! children before parents.

use core::ops::Range;
use core::{iter, mem};

use crate::parser::ast::Kind;
use crate::parser::block::Cells;
use crate::parser::error::ParseError;
use crate::parser::extension::{Construct, Context, Extension, Frame};
use crate::parser::inline::children_mut;
use crate::parser::lexer::BLOCK_CAP;
use crate::parser::line::{Base, line_from, width};
use crate::parser::located::{
    Block, BlockQuote, Code, CodeBlock, Document, Heading, Html, Image, Inline, Item, Line,
    LineBreak, List, MarkdownBlock, Paragraph, Section, Table, TableCell, TableRow, Text,
    ThematicBreak,
};

/// What the pass carries down the tree: the context of the nodes it offers,
/// each grid cell's lines, by the cell's span, and where each line of the
/// source the pass reads starts, in order.
#[derive(Clone, Copy)]
struct Pass<'text> {
    cells: &'text Cells,
    context: Context<'text>,
    starts: &'text [usize],
}

impl<'text> Pass<'text> {
    /// The context of a node whose span starts at `start`: its `column` is
    /// the column `start` is at on its line, or on its cell's line in a grid
    /// cell, once the line is read past the context's frame.
    fn at(&self, start: usize) -> Context<'text> {
        let line_start = self
            .context
            .lines
            .and_then(|lines| lines[..lines.partition_point(|line| line.start <= start)].last())
            .map_or_else(
                || {
                    let starts = &self.starts[..self.starts.partition_point(|&at| at <= start)];
                    starts.last().copied().unwrap_or_default()
                },
                |line| line.start,
            );
        let source = self.context.source;
        let base = Base {
            frame: self.context.frame.columns(),
            ..Base::default()
        };
        let line = line_from(source, line_start, base);
        Context {
            // A span that starts in the white space the frame leaves out, as
            // a markdown block's own span does, starts at the frame's column 0.
            column: if start < line.start {
                0
            } else {
                width(&source[line.start..start], line.column)
            },
            ..self.context
        }
    }

    /// The pass inside a list item or a table cell, one container deeper.
    const fn deeper(&self) -> Self {
        Self {
            context: Context {
                depth: self.context.depth + 1,
                ..self.context
            },
            ..*self
        }
    }

    /// The pass inside the cell that spans `span`: a grid cell's nodes read
    /// only its lines, and its lines start their frame.
    fn inside(&self, span: &Range<usize>) -> Self {
        self.cells.get(span).map_or(*self, |lines| Self {
            context: Context {
                frame: Frame::NONE,
                lines: Some(lines),
                ..self.context
            },
            ..*self
        })
    }
}

/// A node that offers its children, then itself, to the extensions.
trait Offer {
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError>;
}

impl<T> Offer for Block<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        match self {
            Self::BlockQuote(block_quote) => block_quote.offer(pass),
            Self::CodeBlock(code_block) => code_block.offer(pass),
            Self::Html(html) => html.offer(pass),
            Self::List(list) => list.offer(pass),
            Self::Paragraph(paragraph) => paragraph.offer(pass),
            Self::Table(table) => table.offer(pass),
            Self::ThematicBreak(thematic_break) => thematic_break.offer(pass),
        }
    }
}

impl<T> Offer for BlockQuote<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::BlockQuote(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Code<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::Code(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for CodeBlock<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::CodeBlock(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Document<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        self.body.offer(pass)?;
        let claimed = T::claim(Construct::Document(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Heading<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.children, pass)?;
        let claimed = T::claim(Construct::Heading(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Html<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::Html(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Image<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::Image(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Inline<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        if let Some(children) = children_mut(self) {
            *children = inlines(mem::take(children), pass)?;
        }
        own(self, pass)
    }
}

impl<T> Offer for Item<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        self.body.offer(&pass.deeper())?;
        let claimed = T::claim(Construct::Item(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Line<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.children, pass)?;
        let claimed = T::claim(Construct::Line(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for LineBreak<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::LineBreak(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for List<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.items, pass)?;
        let claimed = T::claim(Construct::List(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for MarkdownBlock<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        if pass.context.depth >= BLOCK_CAP {
            return Ok(());
        }
        each(&mut self.sections, pass)?;
        let claimed = T::claim(Construct::MarkdownBlock(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Paragraph<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.lines, pass)?;
        let claimed = T::claim(Construct::Paragraph(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Section<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        if let Some(heading) = &mut self.heading {
            heading.offer(pass)?;
        }
        each(&mut self.blocks, pass)?;
        each(&mut self.subsections, pass)?;
        let claimed = T::claim(Construct::Section(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Table<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.head, pass)?;
        each(&mut self.rows, pass)?;
        let claimed = T::claim(Construct::Table(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for TableCell<T>
where
    T: Extension,
{
    /// A grid cell, and every node inside it, is offered with the cell's
    /// lines.
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let inside = pass.inside(&self.span);
        self.body.offer(&inside.deeper())?;
        let claimed = T::claim(Construct::TableCell(self), &inside.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for TableRow<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        each(&mut self.cells, pass)?;
        let claimed = T::claim(Construct::TableRow(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for Text<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::Text(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

impl<T> Offer for ThematicBreak<T>
where
    T: Extension,
{
    fn offer(&mut self, pass: &Pass<'_>) -> Result<(), ParseError> {
        let claimed = T::claim(Construct::ThematicBreak(self), &pass.at(self.span.start));
        settle(&mut self.kind, claimed, pass)
    }
}

/// Offers every node of `document`, the document last. `cells` holds each
/// grid cell's lines.
pub fn document<T>(
    document: &mut Document<T>,
    context: &Context<'_>,
    cells: &Cells,
) -> Result<(), ParseError>
where
    T: Extension,
{
    document.offer(&Pass {
        cells,
        context: *context,
        starts: &line_starts(context.source, &document.span),
    })
}

/// Offers every node of `block`, the block last. `cells` holds each grid
/// cell's lines.
pub fn markdown_block<T>(
    block: &mut MarkdownBlock<T>,
    context: &Context<'_>,
    cells: &Cells,
) -> Result<(), ParseError>
where
    T: Extension,
{
    block.offer(&Pass {
        cells,
        context: *context,
        starts: &line_starts(context.source, &block.span),
    })
}

fn each<N>(nodes: &mut [N], pass: &Pass<'_>) -> Result<(), ParseError>
where
    N: Offer,
{
    nodes.iter_mut().try_for_each(|node| node.offer(pass))
}

/// Offers each of `nodes` and every node inside them, children before their
/// parent, and gives them back. A node waits on a stack, its children taken
/// out of it, while they are offered, so nesting takes no call depth.
fn inlines<T>(nodes: Vec<Inline<T>>, pass: &Pass<'_>) -> Result<Vec<Inline<T>>, ParseError>
where
    T: Extension,
{
    // Each waiting node, with its siblings offered before it and those still
    // to offer after it.
    let mut waiting = Vec::new();
    let mut offered = Vec::new();
    let mut rest = popping(nodes);
    loop {
        if let Some(mut node) = rest.pop() {
            if let Some(children) = children_mut(&mut node).map(mem::take) {
                let before = mem::take(&mut offered);
                let after = mem::replace(&mut rest, popping(children));
                waiting.push((node, before, after));
            } else {
                own(&mut node, pass)?;
                offered.push(node);
            }
        } else if let Some((mut node, before, after)) = waiting.pop() {
            let children = mem::replace(&mut offered, before);
            if let Some(slot) = children_mut(&mut node) {
                *slot = children;
            }
            rest = after;
            own(&mut node, pass)?;
            offered.push(node);
        } else {
            return Ok(offered);
        }
    }
}

/// Where each line of `source` that `span` lies on starts, in order.
fn line_starts(source: &str, span: &Range<usize>) -> Vec<usize> {
    let first = source[..span.start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let rest = source[span.clone()]
        .match_indices('\n')
        .map(|(newline, _)| span.start + newline + 1);
    iter::once(first).chain(rest).collect()
}

/// Offers `node` alone, its children already offered.
fn own<T>(node: &mut Inline<T>, pass: &Pass<'_>) -> Result<(), ParseError>
where
    T: Extension,
{
    match node {
        Inline::Code(code) => code.offer(pass),
        Inline::Emphasis(emphasis) => {
            let claimed = T::claim(Construct::Emphasis(emphasis), &pass.at(emphasis.span.start));
            settle(&mut emphasis.kind, claimed, pass)
        }
        Inline::Html(html) => html.offer(pass),
        Inline::Image(image) => image.offer(pass),
        Inline::LineBreak(line_break) => line_break.offer(pass),
        Inline::Link(link) => {
            let claimed = T::claim(Construct::Link(link), &pass.at(link.span.start));
            settle(&mut link.kind, claimed, pass)
        }
        Inline::Strong(strong) => {
            let claimed = T::claim(Construct::Strong(strong), &pass.at(strong.span.start));
            settle(&mut strong.kind, claimed, pass)
        }
        Inline::Text(text) => text.offer(pass),
    }
}

/// `nodes` in reverse, so that popping gives them in order.
fn popping<T>(mut nodes: Vec<Inline<T>>) -> Vec<Inline<T>>
where
    T: Extension,
{
    nodes.reverse();
    nodes
}

/// Puts a claimed node into `kind`. A claim that fails leaves the plain
/// node, or fails a strict parse.
fn settle<T>(
    kind: &mut Kind<T>,
    claimed: Option<Result<T::Node<T>, ParseError>>,
    pass: &Pass<'_>,
) -> Result<(), ParseError>
where
    T: Extension,
{
    match claimed {
        Some(Ok(node)) => {
            kind.0 = Some(Box::new(node));
            Ok(())
        }
        Some(Err(error)) if pass.context.strict => Err(error),
        None | Some(Err(_)) => Ok(()),
    }
}
