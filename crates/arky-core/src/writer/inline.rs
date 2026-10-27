//! Inline nodes written on a line: markup as arky writes it, and text with
//! a backslash before each character that would otherwise read as markup.

use core::fmt::{self, Write as _};
use core::mem;

use crate::parser::ast::{
    Code, Continues, Html, Image, Inline, Kind, LineBreak, Link, LinkForm, LinkInline, Text,
};
use crate::parser::definition;
use crate::parser::entity;
use crate::parser::extension::{Extension, Written};
use crate::parser::html;
use crate::parser::inline::{is_punctuation, is_whitespace};
use crate::parser::lexer;
use crate::parser::line::{Base, space};
use crate::parser::table;
use crate::writer::Writer;

/// The mark on each side of emphasis.
pub const EMPHASIS: &str = "*";

/// The mark on each side of strong text.
pub const STRONG: &str = "**";

/// How a line break no extension claimed is written.
#[derive(Clone, Copy)]
enum Breaks {
    /// `\`, where the line ends after it, as in a paragraph.
    Backslash,
    /// `\`, then the line ends: a heading written over several lines.
    Cut,
    /// `&#10;`: a heading written on one line.
    Reference,
}

/// What is around a text segment: the characters before and after it, and
/// whether its first and its last character are written as references.
#[derive(Clone, Copy)]
struct Ends {
    after: Near,
    before: Near,
    referred: Referred,
}

/// What is written next to a text: a character, the start or the end of its
/// line, or nothing known, for a node written on its own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Near {
    Char(char),
    Edge,
    Unknown,
}

/// The backticks around a code span, the same for every piece of a span
/// written over several lines, and whether a space pads the content inside
/// them.
#[derive(Clone, Copy)]
struct Fence {
    pad: bool,
    ticks: usize,
}

/// The code span pieces of a paragraph or a heading, in the order they are
/// written: each piece's place among the whole spans, and each whole span's
/// content.
#[derive(Default)]
pub struct Pieces {
    open: bool,
    owners: Vec<usize>,
    spans: Vec<String>,
}

/// Whether the first and the last character of a text segment are written
/// as character references.
#[derive(Clone, Copy, Default)]
struct Referred {
    first: bool,
    last: bool,
}

/// A part of a line: an emphasis or strong mark, which opens when `opens` is
/// set and closes otherwise; markup, written as it is; or text, escaped by
/// what is around it. `link` marks text between a link's brackets.
enum Segment {
    Mark { mark: &'static str, opens: bool },
    Markup(String),
    Text { link: bool, value: String },
}

/// The segments of the line being written, and the fence of each code span
/// piece still to write, the next one last.
pub struct Segments {
    /// Whether the line starts with inline HTML or a code span piece, which
    /// a backslash cannot guard.
    bare_led: bool,
    breaks: Breaks,
    /// The lines that a line break ended, each with whether it starts with
    /// text, when lines are cut at them.
    cut: Vec<(String, bool)>,
    fences: Vec<Fence>,
    list: Vec<Segment>,
    /// Whether every `|` of the text is escaped.
    pipes: bool,
}

/// What a run of `*` or `_` has on one side.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Mark,
    Other,
    Space,
}

/// A node of a line, or of a link's text, as the writer reads it.
pub trait InlineNode<T>: Sized
where
    T: Extension,
{
    fn view(&self) -> View<'_, T, Self>;
}

/// An inline node, its children typed as the node that holds them.
pub enum View<'node, T, N>
where
    T: Extension,
{
    Code(&'node Code<T>),
    Emphasis {
        children: &'node [N],
        continues: Continues,
        kind: &'node Kind<T>,
        plain: Written<'node, T>,
    },
    Html(&'node Html<T>),
    Image(&'node Image<T>),
    LineBreak(&'node LineBreak<T>),
    Link(&'node Link<T>),
    Strong {
        children: &'node [N],
        continues: Continues,
        kind: &'node Kind<T>,
        plain: Written<'node, T>,
    },
    Text(&'node Text<T>),
}

impl Fence {
    /// A run of backticks longer than any inside `value`, padded when the
    /// value starts or ends with a backtick, or starts and ends with a
    /// space and is not all spaces, since the reader strips one space from
    /// each end.
    fn of(value: &str) -> Self {
        let blank = value.bytes().all(|byte| matches!(byte, b' ' | b'\n'));
        let pad = value.starts_with('`')
            || value.ends_with('`')
            || (!blank && value.starts_with(' ') && value.ends_with(' '));
        Self {
            pad,
            ticks: super::longest_run(value, '`') + 1,
        }
    }
}

impl<T> InlineNode<T> for Inline<T>
where
    T: Extension,
{
    fn view(&self) -> View<'_, T, Self> {
        match self {
            Self::Code(code) => View::Code(code),
            Self::Emphasis(emphasis) => View::Emphasis {
                children: &emphasis.children,
                continues: emphasis.continues,
                kind: &emphasis.kind,
                plain: Written::Emphasis(emphasis),
            },
            Self::Html(html) => View::Html(html),
            Self::Image(image) => View::Image(image),
            Self::LineBreak(line_break) => View::LineBreak(line_break),
            Self::Link(link) => View::Link(link),
            Self::Strong(strong) => View::Strong {
                children: &strong.children,
                continues: strong.continues,
                kind: &strong.kind,
                plain: Written::Strong(strong),
            },
            Self::Text(text) => View::Text(text),
        }
    }
}

impl<T> InlineNode<T> for LinkInline<T>
where
    T: Extension,
{
    fn view(&self) -> View<'_, T, Self> {
        match self {
            Self::Code(code) => View::Code(code),
            Self::Emphasis(emphasis) => View::Emphasis {
                children: &emphasis.children,
                continues: emphasis.continues,
                kind: &emphasis.kind,
                plain: Written::LinkEmphasis(emphasis),
            },
            Self::Html(html) => View::Html(html),
            Self::Image(image) => View::Image(image),
            Self::LineBreak(line_break) => View::LineBreak(line_break),
            Self::Strong(strong) => View::Strong {
                children: &strong.children,
                continues: strong.continues,
                kind: &strong.kind,
                plain: Written::LinkStrong(strong),
            },
            Self::Text(text) => View::Text(text),
        }
    }
}

impl Pieces {
    /// Adds the code span pieces among `nodes` and their children.
    pub fn add<T, N>(&mut self, nodes: &[N])
    where
        T: Extension,
        N: InlineNode<T>,
    {
        for node in nodes {
            match node.view() {
                View::Code(code) => {
                    if code.kind.0.is_none() {
                        self.code(&code.value, code.continues);
                    }
                }
                View::Emphasis {
                    children,
                    continues: _,
                    kind,
                    plain: _,
                }
                | View::Strong {
                    children,
                    continues: _,
                    kind,
                    plain: _,
                } => {
                    if kind.0.is_none() {
                        self.add(children);
                    }
                }
                View::Link(link) => {
                    if link.kind.0.is_none() && link.form == LinkForm::Markdown {
                        self.add(&link.children);
                    }
                }
                View::Html(_) | View::Image(_) | View::LineBreak(_) | View::Text(_) => {}
            }
        }
    }

    /// Adds a code span piece holding `value`. A piece that goes on from the
    /// line before joins the span it is part of.
    pub fn code(&mut self, value: &str, continues: Continues) {
        let from_before = matches!(continues, Continues::Both | Continues::Previous);
        match self.spans.last_mut() {
            Some(span) if from_before && self.open => {
                span.push('\n');
                span.push_str(value);
            }
            Some(_) | None => self.spans.push(value.to_owned()),
        }
        self.owners.push(self.spans.len().saturating_sub(1));
        self.open = matches!(continues, Continues::Both | Continues::Next);
    }

    /// The fence of each piece, the first one last.
    fn fences(&self) -> Vec<Fence> {
        self.owners
            .iter()
            .rev()
            .map(|&owner| Fence::of(&self.spans[owner]))
            .collect()
    }

    /// The code span pieces among `nodes`.
    pub fn of<T, N>(nodes: &[N]) -> Self
    where
        T: Extension,
        N: InlineNode<T>,
    {
        let mut pieces = Self::default();
        pieces.add(nodes);
        pieces
    }
}

impl Segment {
    /// The character the segment starts with, or ends with when `last` is
    /// set.
    fn edge(&self, last: bool) -> Near {
        let text: &str = match self {
            Self::Mark { mark, opens: _ } => mark,
            Self::Markup(markup) => markup,
            Self::Text { link: _, value } => value,
        };
        let found = if last {
            text.chars().next_back()
        } else {
            text.chars().next()
        };
        found.map_or(Near::Unknown, Near::Char)
    }
}

impl Segments {
    /// A code span piece holding `value`: the opening backticks before the
    /// first piece, the closing ones after the last.
    pub fn code(&mut self, value: &str, continues: Continues) {
        self.bare_led |= self.list.is_empty();
        let fence = self.fences.pop().unwrap_or_else(|| Fence::of(value));
        let ticks = "`".repeat(fence.ticks);
        let pad = if fence.pad { " " } else { "" };
        let mut written = String::new();
        if opens(continues) {
            written.push_str(&ticks);
            written.push_str(pad);
        }
        written.push_str(value);
        if closes(continues) {
            written.push_str(pad);
            written.push_str(&ticks);
        }
        self.markup(&written);
    }

    /// Ends the line after the `\` of every line break written from now on.
    pub const fn cut_at_line_breaks(&mut self) {
        self.breaks = Breaks::Cut;
    }

    /// `*emphasis*`, holding `children`.
    pub fn emphasis<T, N>(
        &mut self,
        writer: &mut Writer,
        continues: Continues,
        children: &[N],
        link: bool,
    ) where
        T: Extension,
        N: InlineNode<T>,
    {
        self.wrapped(continues, EMPHASIS, |segments| {
            segments.nodes(writer, children, link);
        });
    }

    /// Puts a backslash before every `|` of the text written from now on.
    pub const fn escape_pipes(&mut self) {
        self.pipes = true;
    }

    /// Inline HTML, as written.
    pub fn html(&mut self, value: &str) {
        self.bare_led |= self.list.is_empty();
        self.markup(value);
    }

    /// `![alt](destination "title")`, or `![[destination|alt]]` for a wiki
    /// embed.
    pub fn image(
        &mut self,
        form: LinkForm,
        continues: Continues,
        alt: &str,
        destination: &str,
        title: Option<&str>,
    ) {
        let mut written = String::new();
        match form {
            LinkForm::Markdown => {
                if opens(continues) {
                    written.push_str("![");
                }
                written.push_str(alt);
                if closes(continues) {
                    written.push_str(&target(destination, title));
                }
            }
            LinkForm::Wiki => {
                written.push_str("![[");
                written.push_str(destination);
                if !alt.is_empty() {
                    written.push('|');
                    written.push_str(alt);
                }
                written.push_str("]]");
            }
        }
        self.markup(&written);
    }

    /// Whether the segment at `at` is text.
    fn is_text(&self, at: Option<usize>) -> bool {
        matches!(
            at.and_then(|index| self.list.get(index)),
            Some(Segment::Text { link: _, value: _ })
        )
    }

    /// The line written, with a backslash before a first character that
    /// would start a block, or four spaces when inline HTML or code leads it
    /// and it is not `first`, a paragraph's first; and whether text leads it.
    pub fn line(&mut self, first: bool) -> (String, bool) {
        let text_led = matches!(self.list.first(), Some(Segment::Text { link: _, value: _ }));
        let bare_led = self.bare_led;
        let mut text = self.take(Near::Edge, Near::Edge);
        if text_led && let Some(at) = block_start(&text, first) {
            text.insert(at, '\\');
        }
        if bare_led && !first && starts_block(&text) {
            text.insert_str(0, "    ");
        }
        (text, text_led)
    }

    /// A line break: `\`, which ends the line when lines are cut at line
    /// breaks, or `&#10;`.
    pub fn line_break(&mut self) {
        match self.breaks {
            Breaks::Backslash => self.markup("\\"),
            Breaks::Cut => {
                self.markup("\\");
                let line = self.line(self.cut.is_empty());
                self.cut.push(line);
            }
            Breaks::Reference => self.markup("&#10;"),
        }
    }

    /// Writes every line break from now on as `&#10;`.
    pub const fn line_breaks_as_references(&mut self) {
        self.breaks = Breaks::Reference;
    }

    /// The lines written, each ended by a line break but the last, each with
    /// the guard of `line`, when lines are cut at line breaks, and whether
    /// the first starts with text.
    pub fn lines(&mut self) -> (Vec<String>, bool) {
        let last = self.line(self.cut.is_empty());
        self.cut.push(last);
        let text_led = self.cut.first().is_some_and(|&(_, led)| led);
        let lines = mem::take(&mut self.cut)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        (lines, text_led)
    }

    /// `[text](target "title")`, or `[[target|text]]` for a wiki link.
    pub fn link<T>(&mut self, writer: &mut Writer, link: &Link<T>)
    where
        T: Extension,
    {
        match link.form {
            LinkForm::Markdown => self.markdown_link(
                link.continues,
                &link.target,
                link.title.as_deref(),
                |segments| segments.nodes(writer, &link.children, true),
            ),
            LinkForm::Wiki => {
                let alias: String = link.children.iter().filter_map(text_of).collect();
                self.wiki_link(&link.target, &alias);
            }
        }
    }

    /// Adds `mark`, an emphasis or strong mark that opens when `opens` is set.
    fn mark(&mut self, mark: &'static str, opens: bool) {
        if !mark.is_empty() {
            self.list.push(Segment::Mark { mark, opens });
        }
    }

    /// `[text](destination "title")`, also for a reference link or an autolink. The `[` goes
    /// before the first piece of a link written over several lines, the rest after the last.
    pub fn markdown_link<F>(
        &mut self,
        continues: Continues,
        destination: &str,
        title: Option<&str>,
        children: F,
    ) where
        F: FnOnce(&mut Self),
    {
        if opens(continues) {
            self.markup("[");
        }
        children(self);
        if closes(continues) {
            self.markup(&target(destination, title));
        }
    }

    /// Adds `text`, written as it is.
    pub fn markup(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.list.last_mut() {
            Some(Segment::Markup(last)) => last.push_str(text),
            Some(Segment::Mark { mark: _, opens: _ } | Segment::Text { link: _, value: _ })
            | None => {
                self.list.push(Segment::Markup(text.to_owned()));
            }
        }
    }

    pub fn new(pieces: &Pieces) -> Self {
        Self {
            bare_led: false,
            breaks: Breaks::Backslash,
            cut: Vec::new(),
            fences: pieces.fences(),
            list: Vec::new(),
            pipes: false,
        }
    }

    /// Adds `node`: what the extension that claimed it writes, or else its
    /// plain form. `link` is set between a link's brackets.
    pub fn node<T, N>(&mut self, writer: &mut Writer, node: &N, link: bool)
    where
        T: Extension,
        N: InlineNode<T>,
    {
        let (kind, plain) = match node.view() {
            View::Code(code) => (&code.kind, Written::Code(code)),
            View::Emphasis {
                children: _,
                continues: _,
                kind,
                plain,
            }
            | View::Strong {
                children: _,
                continues: _,
                kind,
                plain,
            } => (kind, plain),
            View::Html(html) => (&html.kind, Written::Html(html)),
            View::Image(image) => (&image.kind, Written::Image(image)),
            View::LineBreak(line_break) => (&line_break.kind, Written::LineBreak(line_break)),
            View::Link(anchor) => (&anchor.kind, Written::Link(anchor)),
            View::Text(text) => (&text.kind, Written::Text(text)),
        };
        match kind.0.as_deref() {
            Some(claimed) => self.markup(&writer.claimed(claimed, plain)),
            None => self.plain(writer, node, link),
        }
    }

    /// Adds `nodes`, each run of plain emphases or of plain strongs next to
    /// each other and whole on their line inside one pair of marks.
    pub fn nodes<T, N>(&mut self, writer: &mut Writer, nodes: &[N], link: bool)
    where
        T: Extension,
        N: InlineNode<T>,
    {
        let touching = |left: &N, right: &N| {
            whole_span(left)
                .zip(whole_span(right))
                .is_some_and(|((left_mark, _), (right_mark, _))| left_mark == right_mark)
        };
        for run in nodes.chunk_by(touching) {
            match run {
                [node] => self.node(writer, node, link),
                _ => self.touching(writer, run, link),
            }
        }
    }

    fn plain<T, N>(&mut self, writer: &mut Writer, node: &N, link: bool)
    where
        T: Extension,
        N: InlineNode<T>,
    {
        match node.view() {
            View::Code(code) => self.code(&code.value, code.continues),
            View::Emphasis {
                children,
                continues,
                kind: _,
                plain: _,
            } => self.emphasis(writer, continues, children, link),
            View::Html(html) => self.html(&html.value),
            View::Image(image) => self.image(
                image.form,
                image.continues,
                &image.alt,
                &image.target,
                image.title.as_deref(),
            ),
            View::LineBreak(_) => self.line_break(),
            View::Link(anchor) => self.link(writer, anchor),
            View::Strong {
                children,
                continues,
                kind: _,
                plain: _,
            } => self.strong(writer, continues, children, link),
            View::Text(text) => self.text(&text.value, link),
        }
    }

    /// For each segment, which of its edge characters are written as character
    /// references, so that each run of marks opens or closes where it stands.
    fn referred(&self, start: Near, end: Near) -> Vec<Referred> {
        let mut referred = vec![Referred::default(); self.list.len()];
        let mut index = 0;
        while index < self.list.len() {
            if !matches!(self.list[index], Segment::Mark { mark: _, opens: _ }) {
                index += 1;
                continue;
            }
            let first = index;
            while matches!(
                self.list.get(index),
                Some(Segment::Mark { mark: _, opens: _ })
            ) {
                index += 1;
            }
            let run = &self.list[first..index];
            let previous = first.checked_sub(1);
            let next = (index < self.list.len()).then_some(index);
            let before = previous.map_or(start, |at| self.list[at].edge(true));
            let after = next.map_or(end, |at| self.list[at].edge(false));
            let opens = run.iter().any(|mark| {
                matches!(
                    mark,
                    Segment::Mark {
                        mark: _,
                        opens: true
                    }
                )
            });
            let closes = run.iter().any(|mark| {
                matches!(
                    mark,
                    Segment::Mark {
                        mark: _,
                        opens: false
                    }
                )
            });
            let mut left = side(before, '*');
            let mut right = side(after, '*');
            // A space on the inside: before a closing mark, after an opening one.
            if closes && left == Some(Side::Space) && self.is_text(previous) {
                referred[first - 1].last = true;
                left = Some(Side::Mark);
            }
            if opens && right == Some(Side::Space) && self.is_text(next) {
                referred[index].first = true;
                right = Some(Side::Mark);
            }
            // Punctuation on the inside needs a space or punctuation on the outside.
            if opens
                && right == Some(Side::Mark)
                && left == Some(Side::Other)
                && self.is_text(previous)
            {
                referred[first - 1].last = true;
                left = Some(Side::Mark);
            }
            if closes
                && left == Some(Side::Mark)
                && right == Some(Side::Other)
                && self.is_text(next)
            {
                referred[index].first = true;
            }
        }
        referred
    }

    /// `**strong**`, holding `children`.
    pub fn strong<T, N>(
        &mut self,
        writer: &mut Writer,
        continues: Continues,
        children: &[N],
        link: bool,
    ) where
        T: Extension,
        N: InlineNode<T>,
    {
        self.wrapped(continues, STRONG, |segments| {
            segments.nodes(writer, children, link);
        });
    }

    /// The segments written between `start` and `end`, which are taken out.
    pub fn take(&mut self, start: Near, end: Near) -> String {
        let referred = self.referred(start, end);
        let mut out = String::new();
        for (index, segment) in self.list.iter().enumerate() {
            match segment {
                Segment::Mark { mark, opens: _ } => out.push_str(mark),
                Segment::Markup(markup) => out.push_str(markup),
                Segment::Text { link, value } => {
                    let before = index
                        .checked_sub(1)
                        .and_then(|previous| self.list.get(previous))
                        .map_or(start, |previous| previous.edge(true));
                    let after = self
                        .list
                        .get(index + 1)
                        .map_or(end, |next| next.edge(false));
                    let ends = Ends {
                        after,
                        before,
                        referred: referred[index],
                    };
                    escape(&mut out, value, ends, *link, self.pipes);
                }
            }
        }
        self.list.clear();
        self.bare_led = false;
        out
    }

    /// Adds text, to be escaped by what is around it.
    pub fn text(&mut self, value: &str, link: bool) {
        if value.is_empty() {
            return;
        }
        match self.list.last_mut() {
            Some(Segment::Text {
                link: last_link,
                value: last,
            }) if *last_link == link => last.push_str(value),
            Some(
                Segment::Mark { mark: _, opens: _ }
                | Segment::Markup(_)
                | Segment::Text { link: _, value: _ },
            )
            | None => {
                self.list.push(Segment::Text {
                    link,
                    value: value.to_owned(),
                });
            }
        }
    }

    /// `run`, plain emphases or plain strongs next to each other, inside one
    /// pair of their mark, the children of each one after the other.
    fn touching<T, N>(&mut self, writer: &mut Writer, run: &[N], link: bool)
    where
        T: Extension,
        N: InlineNode<T>,
    {
        let spans = run.iter().filter_map(whole_span);
        let mark = spans.clone().next().map_or("", |(mark, _)| mark);
        self.wrapped(Continues::Neither, mark, |segments| {
            for (_, children) in spans {
                segments.nodes(writer, children, link);
            }
        });
    }

    /// `[[destination|alias]]`, a wiki link.
    pub fn wiki_link(&mut self, destination: &str, alias: &str) {
        let mut written = format!("[[{destination}");
        if !alias.is_empty() {
            written.push('|');
            written.push_str(alias);
        }
        written.push_str("]]");
        self.markup(&written);
    }

    /// What `children` writes between two `mark`s: the opening one before
    /// the first piece of a node written over several lines, the closing one
    /// after the last. A whole node that writes nothing is written as nothing.
    pub fn wrapped<F>(&mut self, continues: Continues, mark: &'static str, children: F)
    where
        F: FnOnce(&mut Self),
    {
        if opens(continues) {
            self.mark(mark, true);
        }
        let written = (self.list.len(), self.cut.len());
        children(self);
        let nothing = (self.list.len(), self.cut.len()) == written;
        if opens(continues) && closes(continues) && !mark.is_empty() && nothing {
            let _: Option<Segment> = self.list.pop();
            return;
        }
        if closes(continues) {
            self.mark(mark, false);
        }
    }
}

/// Puts a backslash before the first `#` of a final run of `#`s in `text`,
/// a heading's text written after its `#` marks, when a space or a tab
/// precedes the run or the run is the whole text, since the reader would
/// take it for the heading's closing sequence.
pub fn guard_closing_sequence(text: &mut String) {
    let before_run = text.trim_end_matches('#');
    let closes = before_run.len() < text.len()
        && (before_run.is_empty() || before_run.ends_with([' ', '\t']));
    if closes {
        text.insert(before_run.len(), '\\');
    }
}

/// Puts a backslash before the `[` that starts the first of `lines`, a
/// paragraph that starts with text, when the paragraph would read as a link
/// reference definition.
pub fn guard_definition(lines: &mut [String]) {
    let defines = lines.first().is_some_and(|first| first.starts_with('['))
        && definition::read(&lines.join("\n")).definition.is_some();
    if defines && let Some(first) = lines.first_mut() {
        first.insert(0, '\\');
    }
}

/// Whether `nodes` hold a line break no extension claimed, among them or
/// among the children of a plain emphasis, strong or link they hold.
pub fn holds_line_break<T, N>(nodes: &[N]) -> bool
where
    T: Extension,
    N: InlineNode<T>,
{
    nodes.iter().any(|node| match node.view() {
        View::Emphasis {
            children,
            continues: _,
            kind,
            plain: _,
        }
        | View::Strong {
            children,
            continues: _,
            kind,
            plain: _,
        } => kind.0.is_none() && holds_line_break(children),
        View::LineBreak(line_break) => line_break.kind.0.is_none(),
        View::Link(link) => {
            link.kind.0.is_none()
                && link.form == LinkForm::Markdown
                && holds_line_break(&link.children)
        }
        View::Code(_) | View::Html(_) | View::Image(_) | View::Text(_) => false,
    })
}

/// Three or more of the same `-`, `*` or `_`, with nothing but spaces and
/// tabs between them.
pub fn is_thematic_break(text: &str) -> bool {
    text.chars()
        .next()
        .filter(|mark| matches!(mark, '-' | '*' | '_'))
        .is_some_and(|mark| {
            text.chars()
                .all(|character| character == mark || matches!(character, ' ' | '\t'))
                && text.chars().filter(|&character| character == mark).count() >= 3
        })
}

/// Whether `lines`, a paragraph as written, would read as a table that
/// starts on one of them, each line offered to the table reader as the
/// lexer offers it.
pub fn reads_as_table(lines: &[String]) -> bool {
    let source = lines.join("\n");
    let mut line_start = 0;
    for line in lines {
        let indent = space(line, 0);
        let start = line_start + indent.bytes;
        let end = start + line[indent.bytes..].trim_end_matches([' ', '\t']).len();
        let next = (line_start + line.len() + 1).min(source.len());
        let read = table::read(
            &source,
            line_start,
            indent.columns,
            start..end,
            next,
            0,
            Base::default(),
        );
        if read.is_some() {
            return true;
        }
        line_start = next;
    }
    false
}

/// Where a backslash goes so that `text`, a line that starts with text, does
/// not start a block: before the first character of a heading, a block
/// quote, a bullet, a thematic break, a tilde fence or, after the first line
/// of a paragraph, a heading underline; before the `.` or `)` of an ordered
/// list's number.
fn block_start(text: &str, first: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let marks = bytes.iter().take_while(|&&byte| byte == b'#').count();
    let heading = (1..=6).contains(&marks) && matches!(bytes.get(marks), None | Some(b' ' | b'\t'));
    let bullet = matches!(bytes.first(), Some(b'-' | b'+' | b'*'))
        && matches!(bytes.get(1), None | Some(b' ' | b'\t'));
    let underline = !first
        && bytes.first().is_some_and(|&mark| {
            matches!(mark, b'=' | b'-') && bytes.iter().all(|&byte| byte == mark)
        });
    let fence = text.starts_with("~~~");
    if heading || bullet || underline || fence || is_thematic_break(text) || text.starts_with('>') {
        return Some(0);
    }
    let digits = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    ((1..=9).contains(&digits)
        && matches!(bytes.get(digits), Some(b'.' | b')'))
        && matches!(bytes.get(digits + 1), None | Some(b' ' | b'\t')))
    .then_some(digits)
}

/// Whether `text`, written at the start of a line after a paragraph's
/// first, would start a block, also a backtick fence or an HTML block of a
/// kind that interrupts a paragraph.
fn starts_block(text: &str) -> bool {
    block_start(text, false).is_some()
        || lexer::fence(text).is_some()
        || html::start(text).is_some_and(|start| start.interrupts)
}

/// Whether a piece that `continues` closes on its line.
const fn closes(continues: Continues) -> bool {
    matches!(continues, Continues::Neither | Continues::Previous)
}

/// Writes `value` with a backslash before each character that is not to be
/// read as markup: a backslash before punctuation or a line end, a
/// backtick, a `*` or `_` run that could open or close emphasis, a `&` that
/// starts a reference, a `<` that could start a tag or an autolink, and the
/// `!`, `[` and `]` that would start or end a link. Between a link's
/// brackets, `link`, every bracket is escaped, and with `pipes`, every `|`.
/// The spaces and tabs that start or end a line, which the reader drops,
/// every line ending, and the first or last character that `ends` marks
/// are written as character references.
fn escape(out: &mut String, value: &str, ends: Ends, link: bool, pipes: bool) {
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    let blank = |&&(_, character): &&(usize, char)| matches!(character, ' ' | '\t');
    let lead = if ends.before == Near::Edge {
        chars.iter().take_while(blank).count()
    } else {
        0
    };
    let trail = if ends.after == Near::Edge {
        chars.len() - chars[lead..].iter().rev().take_while(blank).count()
    } else {
        chars.len()
    };
    let referred = |index: usize| {
        index < lead
            || index >= trail
            || matches!(chars.get(index), Some(&(_, '\n')))
            || (ends.referred.first && index == 0)
            || (ends.referred.last && index + 1 == chars.len())
    };
    let near = |index: usize| match chars.get(index) {
        Some(_) if referred(index) => Near::Char('&'),
        Some(&(_, character)) => Near::Char(character),
        None => ends.after,
    };
    let mut index = 0;
    while let Some(&(at, character)) = chars.get(index) {
        if referred(index) {
            let _: fmt::Result = write!(out, "&#{};", u32::from(character));
            index += 1;
            continue;
        }
        let previous = index.checked_sub(1).map_or(ends.before, |back| {
            if referred(back) {
                Near::Char(';')
            } else {
                Near::Char(chars[back].1)
            }
        });
        let (end, escaped) = if matches!(character, '*' | '_') {
            let end = index
                + chars[index..]
                    .iter()
                    .take_while(|&&(_, next)| next == character)
                    .count();
            (end, !inert(character, previous, near(end)))
        } else {
            let next = near(index + 1);
            let escaped = match character {
                '\\' => !matches!(next, Near::Char(found) if !found.is_ascii_punctuation()),
                '`' => true,
                '&' => entity::reference(&value[at..]).is_some(),
                '<' => {
                    !matches!(next, Near::Edge)
                        && !matches!(next, Near::Char(found) if is_whitespace(found))
                }
                '[' => link || matches!(next, Near::Char('[') | Near::Unknown),
                ']' => link || matches!(next, Near::Char('(') | Near::Unknown),
                '!' => matches!(next, Near::Char('[') | Near::Unknown),
                '|' => pipes,
                _ => false,
            };
            (index + 1, escaped)
        };
        for &(_, written) in &chars[index..end] {
            if escaped {
                out.push('\\');
            }
            out.push(written);
        }
        index = end;
    }
}

/// Whether a run of `character`, `*` or `_`, between `before` and `after`
/// can neither open nor close emphasis, by the reader's flanking rules, so
/// it reads as text. A side that is not known, or is the same character,
/// counts as one that could.
fn inert(character: char, before: Near, after: Near) -> bool {
    let (Some(left_side), Some(right_side)) = (side(before, character), side(after, character))
    else {
        return false;
    };
    let space_before = left_side == Side::Space;
    let space_after = right_side == Side::Space;
    let mark_before = left_side == Side::Mark;
    let mark_after = right_side == Side::Mark;
    let left = !space_after && (!mark_after || space_before || mark_before);
    let right = !space_before && (!mark_before || space_after || mark_after);
    let (can_open, can_close) = if character == '*' {
        (left, right)
    } else {
        (
            left && (!right || mark_before),
            right && (!left || mark_after),
        )
    };
    !can_open && !can_close
}

/// Whether a piece that `continues` opens on its line.
const fn opens(continues: Continues) -> bool {
    matches!(continues, Continues::Neither | Continues::Next)
}

/// What `near` is to a run of `character`, or `None` when it is not known
/// or is the same character, which would join the run.
fn side(near: Near, character: char) -> Option<Side> {
    match near {
        Near::Char(found) if found == character => None,
        Near::Char(found) if is_whitespace(found) => Some(Side::Space),
        Near::Char(found) if is_punctuation(found) => Some(Side::Mark),
        Near::Char(_) => Some(Side::Other),
        Near::Edge => Some(Side::Space),
        Near::Unknown => None,
    }
}

/// `](destination "title")`, the end of a link or an image. A destination
/// with a space or a control character, one that starts with `<`, or one
/// whose parentheses do not pair up goes between `<` and `>`.
fn target(destination: &str, title: Option<&str>) -> String {
    let mut written = "](".to_owned();
    if destination.is_empty() {
        if title.is_some() {
            written.push_str("<>");
        }
    } else if destination.starts_with('<')
        || destination.bytes().any(|byte| byte <= b' ' || byte == 0x7f)
        || !paired(destination)
    {
        written.push('<');
        escape_marks(&mut written, destination, &['<', '>'], true);
        written.push('>');
    } else {
        escape_marks(&mut written, destination, &[], true);
    }
    if let Some(text) = title {
        written.push_str(" \"");
        escape_marks(&mut written, text, &['"'], true);
        written.push('"');
    }
    written.push(')');
    written
}

/// Writes `text`, a destination or a title, with a backslash before each of
/// `marks`, each `&` that starts a reference, and each backslash before
/// punctuation or the end. With `line_endings`, a line ending is written
/// `&#10;`.
fn escape_marks(out: &mut String, text: &str, marks: &[char], line_endings: bool) {
    let mut chars = text.char_indices().peekable();
    while let Some((at, character)) = chars.next() {
        if line_endings && character == '\n' {
            out.push_str("&#10;");
            continue;
        }
        let escaped = match character {
            '\\' => chars.peek().is_none_or(|&(_, next)| {
                next.is_ascii_punctuation() || (line_endings && next == '\n')
            }),
            '&' => entity::reference(&text[at..]).is_some(),
            _ => marks.contains(&character),
        };
        if escaped {
            out.push('\\');
        }
        out.push(character);
    }
}

/// Whether the parentheses of `destination` pair up, none closing before it
/// opens.
fn paired(destination: &str) -> bool {
    let mut depth = 0_usize;
    for character in destination.chars() {
        match character {
            '(' => depth += 1,
            ')' => match depth.checked_sub(1) {
                Some(inner) => depth = inner,
                None => return false,
            },
            _ => {}
        }
    }
    depth == 0
}

/// The mark and the children of `node`, when it is an emphasis or a strong
/// that no extension claimed and that is whole on its line.
fn whole_span<'node, T, N>(node: &'node N) -> Option<(&'static str, &'node [N])>
where
    T: Extension + 'node,
    N: InlineNode<T>,
{
    let (mark, children, continues, kind) = match node.view() {
        View::Emphasis {
            children,
            continues,
            kind,
            plain: _,
        } => (EMPHASIS, children, continues, kind),
        View::Strong {
            children,
            continues,
            kind,
            plain: _,
        } => (STRONG, children, continues, kind),
        View::Code(_)
        | View::Html(_)
        | View::Image(_)
        | View::LineBreak(_)
        | View::Link(_)
        | View::Text(_) => return None,
    };
    (kind.0.is_none() && continues == Continues::Neither).then_some((mark, children))
}

/// The text of `node`, when it is text.
fn text_of<'node, T, N>(node: &'node N) -> Option<&'node str>
where
    T: Extension + 'node,
    N: InlineNode<T>,
{
    match node.view() {
        View::Text(text) => Some(&text.value),
        View::Code(_)
        | View::Emphasis {
            children: _,
            continues: _,
            kind: _,
            plain: _,
        }
        | View::Html(_)
        | View::Image(_)
        | View::LineBreak(_)
        | View::Link(_)
        | View::Strong {
            children: _,
            continues: _,
            kind: _,
            plain: _,
        } => None,
    }
}
