//! Inline nodes inside the text lines of a paragraph or a heading.

use core::ops::{Range, RangeInclusive};
use core::{iter, mem};
use std::collections::HashMap;

use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory as _};

use crate::parser::ast::{Continues, Kind, LinkForm};
use crate::parser::definition::{self, Definitions, Target, fold_label};
use crate::parser::extension::Extension;
use crate::parser::located::{
    Code, Emphasis, Html, Image, Inline, Line, LineBreak, Link, Strong, Text,
};
use crate::parser::{entity, html};

/// How deep inline nodes nest: deeper ones overflow a 2 MB debug stack.
pub const INLINE_CAP: usize = 1_000;

/// The children of a node being cut at line ends: their pieces cut so far
/// on each line the node lies on, from the first, and those still to cut,
/// the next one last.
struct Cutting<E>
where
    E: Extension,
{
    first: usize,
    rest: Vec<Inline<E>>,
    spread: Vec<Vec<Inline<E>>>,
}

/// What ends a line of a paragraph before the line after it.
#[derive(Clone, Copy)]
enum Ending {
    /// A backslash that ends the line is a hard line break.
    Backslash,
    /// A code span or inline HTML goes on into the next line: the spaces
    /// and tabs before the line ending, up to this place in the source,
    /// belong to it.
    Held(usize),
    /// No hard line break can end the line.
    Soft,
    /// Two or more spaces, up to this place in the source, are a hard line
    /// break.
    Spaces(usize),
}

/// A node the bracket pass read, which the scanner takes as it is.
enum Found {
    /// An autolink: where it ends, past its `>`, and its target.
    Autolink { close: usize, target: String },
    /// A code span: where it ends, past its closing backticks.
    Code(usize),
    /// Inline HTML: where it ends.
    Html(usize),
    /// A link or an image: the `]` that ends its text, where it ends, and
    /// its target.
    Link {
        close: usize,
        end: usize,
        target: Target,
    },
}

/// The lines of a paragraph, `ranges` of `source`, joined by `\n` into
/// `text`, which the scanner reads: where each line sits in `text`, and how
/// it ends. A line that another follows keeps the spaces and tabs before its
/// line ending.
struct Layout<'src> {
    endings: Vec<Ending>,
    /// The nodes the bracket pass read, each by where it starts in `text`:
    /// at its `[` for a link or an image.
    found: HashMap<usize, Found>,
    /// Where each `[`, `]` and `\n` is in `text`.
    marks: Vec<usize>,
    places: Vec<Range<usize>>,
    ranges: &'src [Range<usize>],
    /// Where each run of backticks starts in `text`, by its length.
    runs: HashMap<usize, Vec<usize>>,
    source: &'src str,
    text: String,
}

/// The order of the pieces being paired, or of the runs not yet removed, as
/// links by their places among them. The places taken out of the order
/// between two others keep their links to each other.
struct Links {
    first: Option<usize>,
    next: Vec<Option<usize>>,
    previous: Vec<Option<usize>>,
}

/// What the scanner has read: a node and how deep it nests, or a run of
/// `*` or `_` not yet paired, by its place in the scanner's runs. A pair of
/// runs is an emphasis or a strong emphasis, how deep it nests, and the
/// place of the first of its pieces, which its children are built from once
/// pairing ends.
enum Piece<E>
where
    E: Extension,
{
    Node(Inline<E>, usize),
    Pair(Inline<E>, usize, Option<usize>),
    Run(usize),
}

/// A run of `*` or `_`. `count` and `span` cover the characters not yet
/// paired; `length` is the run as written.
struct Run {
    can_close: bool,
    can_open: bool,
    character: char,
    count: usize,
    /// How deep the deepest piece between this run and the next run not
    /// yet removed nests.
    deepest: usize,
    length: usize,
    /// The run's place among the pieces.
    place: usize,
    span: Range<usize>,
}

/// Where the parts of a `[[target|alias]]` are.
struct Wiki {
    alias: Option<Range<usize>>,
    close: usize,
    target: Range<usize>,
}

/// Reads the text of a layout into nodes whose spans are places in that
/// text.
struct Scanner<'lay, E>
where
    E: Extension,
{
    layout: &'lay Layout<'lay>,
    pieces: Vec<Piece<E>>,
    runs: Vec<Run>,
    text: String,
    text_end: usize,
    text_start: Option<usize>,
}

impl<E> Cutting<E>
where
    E: Extension,
{
    /// Adds `pieces`, each to the line it lies on.
    fn add(&mut self, pieces: Vec<(usize, Inline<E>)>) {
        for (line, piece) in pieces {
            self.spread[line - self.first].push(piece);
        }
    }
}

impl Ending {
    /// How the line whose text ends at `end` in `source` ends: when another
    /// line of its paragraph `follows`, a backslash or two or more spaces
    /// that end it make a hard line break.
    fn of(source: &str, end: usize, follows: bool) -> Self {
        match line_end(source, end).filter(|_| follows) {
            Some(close) if close == end => Self::Backslash,
            Some(close) if source[end..close].ends_with("  ") => Self::Spaces(close),
            Some(_) | None => Self::Soft,
        }
    }
}

impl Found {
    /// Where the node ends.
    const fn end(&self) -> usize {
        match self {
            Self::Autolink {
                close: end,
                target: _,
            }
            | Self::Code(end)
            | Self::Html(end)
            | Self::Link {
                close: _,
                end,
                target: _,
            } => *end,
        }
    }
}

impl<'src> Layout<'src> {
    /// Where the code span whose opening run of `ticks` backticks is at `at`
    /// ends: past the next run of exactly as many.
    fn code_end(&self, at: usize, ticks: usize) -> Option<usize> {
        let starts = self.runs.get(&ticks)?;
        let next = starts.get(starts.partition_point(|&start| start < at + ticks))?;
        Some(next + ticks)
    }

    /// The pieces of `node`, read from `text`, one for each line it lies
    /// on, by that line, with their spans in the source. The children of an
    /// emphasis, a link or a strong emphasis were taken out of it and cut:
    /// `spread` holds their pieces on each line it lies on.
    fn cut<E>(&self, node: Inline<E>, spread: Vec<Vec<Inline<E>>>) -> Vec<(usize, Inline<E>)>
    where
        E: Extension,
    {
        let read = node.span().clone();
        let mut children = spread.into_iter();
        match node {
            Inline::Code(code) => {
                let mut values = code.value.split('\n');
                self.pieces(&read, |continues, span| {
                    Inline::Code(Code {
                        continues,
                        kind: Kind(None),
                        span,
                        value: values.next().unwrap_or_default().to_owned(),
                    })
                })
            }
            Inline::Emphasis(_) => self.pieces(&read, |continues, span| {
                Inline::Emphasis(Emphasis {
                    children: children.next().unwrap_or_default(),
                    continues,
                    kind: Kind(None),
                    span,
                })
            }),
            Inline::Html(html) => {
                let mut values = html.value.split('\n');
                self.pieces(&read, |continues, span| {
                    Inline::Html(Html {
                        continues,
                        kind: Kind(None),
                        span,
                        value: values.next().unwrap_or_default().to_owned(),
                    })
                })
            }
            Inline::Image(image) => {
                let mut alts = image.alt.split('\n');
                self.pieces(&read, |continues, span| {
                    Inline::Image(Image {
                        alt: alts.next().unwrap_or_default().to_owned(),
                        continues,
                        form: image.form,
                        kind: Kind(None),
                        span,
                        target: image.target.clone(),
                        title: image.title.clone(),
                    })
                })
            }
            Inline::LineBreak(mut line_break) => {
                let line = self.line(read.start);
                // A break of spaces sits at its line end with an empty span.
                line_break.span = if read.is_empty() {
                    self.ranges[line].end..self.end(line)
                } else {
                    self.place(read.start)..self.place(read.end)
                };
                vec![(line, Inline::LineBreak(line_break))]
            }
            Inline::Link(link) => self.pieces(&read, |continues, span| {
                Inline::Link(Link {
                    children: children.next().unwrap_or_default(),
                    continues,
                    form: link.form,
                    kind: Kind(None),
                    span,
                    target: link.target.clone(),
                    title: link.title.clone(),
                })
            }),
            Inline::Strong(_) => self.pieces(&read, |continues, span| {
                Inline::Strong(Strong {
                    children: children.next().unwrap_or_default(),
                    continues,
                    kind: Kind(None),
                    span,
                })
            }),
            Inline::Text(mut text) => {
                text.span = self.place(read.start)..self.place(read.end);
                vec![(self.line(read.start), Inline::Text(text))]
            }
        }
    }

    /// The children `nodes` of a node read at `read` in `text`, none cut
    /// yet.
    fn cutting<E>(&self, nodes: Vec<Inline<E>>, read: &Range<usize>) -> Cutting<E>
    where
        E: Extension,
    {
        let lines = self.lines_of(read);
        let mut rest = nodes;
        rest.reverse();
        Cutting {
            first: *lines.start(),
            rest,
            spread: lines.map(|_| Vec::new()).collect(),
        }
    }

    /// Where the `Line` of `line` ends in the source: past the spaces of a
    /// hard line break that ends it, or of a code span or inline HTML that
    /// goes on from it, or at the end of its text.
    fn end(&self, line: usize) -> usize {
        match self.endings[line] {
            Ending::Held(end) | Ending::Spaces(end) => end,
            Ending::Backslash | Ending::Soft => self.ranges[line].end,
        }
    }

    /// How the line that holds `at`, a place in `text`, ends.
    fn ending(&self, at: usize) -> Ending {
        self.endings[self.line(at)]
    }

    /// The code spans, autolinks, inline HTML, links and images, each read
    /// once, for the scanner to take. Brackets pair by `CommonMark`'s stack
    /// of brackets: once a link, a wiki link or an autolink is read, no `[`
    /// before it opens one. A bracket after a backslash, or inside a code
    /// span, an autolink, inline HTML, a wiki link or embed, or the
    /// destination or label of a link or an image, is in no pair.
    fn find(&self, definitions: &Definitions) -> HashMap<usize, Found> {
        let source = self.text.as_str();
        let bytes = source.as_bytes();
        let end = source.len();
        let mut found = HashMap::new();
        // Each bracket still open, and whether it opens an image. A `[` below
        // `active` in it opens no link.
        let mut open: Vec<(usize, bool)> = Vec::new();
        let mut active = 0;
        let mut searches = html::Searches::default();
        let mut at = 0;
        while at < end {
            at = match bytes[at] {
                b']' => match open.pop() {
                    Some((start, image)) => {
                        let below = open.len();
                        let opens = image || below >= active;
                        active = active.min(below);
                        let read = opens
                            .then(|| link_target(source, definitions, start, at, end))
                            .flatten();
                        match read {
                            Some((target, close)) => {
                                if !image {
                                    active = below;
                                }
                                let link = Found::Link {
                                    close: at,
                                    end: close,
                                    target,
                                };
                                keep(&mut found, start, link)
                            }
                            None => at + 1,
                        }
                    }
                    None => at + 1,
                },
                b'[' => {
                    if let Some(wiki) = self.wiki(at, end) {
                        active = open.len();
                        wiki.close
                    } else {
                        open.push((at, false));
                        at + 1
                    }
                }
                b'!' if bytes.get(at + 1) == Some(&b'[') => self.wiki(at + 1, end).map_or_else(
                    || {
                        open.push((at + 1, true));
                        at + 2
                    },
                    |wiki| wiki.close,
                ),
                b'\\' if at + 1 < end && bytes[at + 1].is_ascii_punctuation() => at + 2,
                b'`' => {
                    let ticks = run(bytes, at, end);
                    self.code_end(at, ticks)
                        .map_or(at + ticks, |close| keep(&mut found, at, Found::Code(close)))
                }
                b'<' => match autolink_at(source, at, end) {
                    Some((target, close)) => {
                        active = open.len();
                        keep(&mut found, at, Found::Autolink { close, target })
                    }
                    None => html::inline_end(source, at, &mut searches)
                        .map_or(at + 1, |close| keep(&mut found, at, Found::Html(close))),
                },
                _ => at + 1,
            };
        }
        found
    }

    /// Ends each line that a code span or inline HTML among `nodes` goes on
    /// from past the spaces and tabs before its line ending, which the node
    /// holds.
    fn hold<E>(&mut self, nodes: &[Inline<E>])
    where
        E: Extension,
    {
        let mut waiting = vec![nodes];
        while let Some(siblings) = waiting.pop() {
            for node in siblings {
                let span = match node {
                    Inline::Code(code) => &code.span,
                    Inline::Html(html) => &html.span,
                    Inline::Emphasis(emphasis) => {
                        waiting.push(&emphasis.children);
                        continue;
                    }
                    Inline::Link(link) => {
                        waiting.push(&link.children);
                        continue;
                    }
                    Inline::Strong(strong) => {
                        waiting.push(&strong.children);
                        continue;
                    }
                    Inline::Image(_) | Inline::LineBreak(_) | Inline::Text(_) => continue,
                };
                let lines = self.lines_of(span);
                for line in *lines.start()..*lines.end() {
                    self.endings[line] = Ending::Held(self.trailing(line).end);
                }
            }
        }
    }

    /// The line that holds `at`, a place in `text`. The `\n` after a line
    /// belongs to it.
    fn line(&self, at: usize) -> usize {
        self.places
            .partition_point(|place| place.start <= at)
            .saturating_sub(1)
    }

    /// Where the text of the line that holds `at`, a place in `text`, ends,
    /// before the spaces and tabs at its end, and where the line ends.
    fn line_ends(&self, at: usize) -> (usize, usize) {
        let line = self.line(at);
        let place = &self.places[line];
        (place.start + self.ranges[line].len(), place.end)
    }

    /// The lines of the layout, each with its pieces of `nodes`.
    fn lines<E>(&self, nodes: Vec<Inline<E>>) -> Vec<Line<E>>
    where
        E: Extension,
    {
        self.spread(nodes, &(0..self.text.len()))
            .into_iter()
            .zip(self.ranges)
            .enumerate()
            .map(|(line, (children, range))| Line {
                children,
                kind: Kind(None),
                span: range.start..self.end(line),
            })
            .collect()
    }

    /// The first and the last line that `span`, in `text`, lies on.
    fn lines_of(&self, span: &Range<usize>) -> RangeInclusive<usize> {
        let last = span.end.saturating_sub(1).max(span.start);
        self.line(span.start)..=self.line(last)
    }

    fn new(source: &'src str, ranges: &'src [Range<usize>], definitions: &Definitions) -> Self {
        let mut text = String::new();
        let mut places = Vec::with_capacity(ranges.len());
        let mut endings = Vec::with_capacity(ranges.len());
        for (index, range) in ranges.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            let start = text.len();
            text.push_str(&source[extent(source, ranges, index)]);
            places.push(start..text.len());
            endings.push(Ending::of(source, range.end, index + 1 < ranges.len()));
        }
        let marks = text
            .bytes()
            .enumerate()
            .filter(|&(_, byte)| matches!(byte, b'[' | b']' | b'\n'))
            .map(|(at, _)| at)
            .collect();
        let mut layout = Self {
            endings,
            found: HashMap::new(),
            marks,
            places,
            ranges,
            runs: backtick_runs(&text),
            source,
            text,
        };
        layout.found = layout.find(definitions);
        layout
    }

    /// The node the bracket pass read at `at`, when it ends by `end`.
    fn node_at(&self, at: usize, end: usize) -> Option<&Found> {
        self.found.get(&at).filter(|node| node.end() <= end)
    }

    /// The pieces of the node read at `read` in `text`, one for each line it
    /// lies on, by that line. `build` makes each piece from how it goes on
    /// and its span in the source: from the node's start, or the line's, to
    /// the node's end, or the line's.
    fn pieces<E, F>(&self, read: &Range<usize>, mut build: F) -> Vec<(usize, Inline<E>)>
    where
        E: Extension,
        F: FnMut(Continues, Range<usize>) -> Inline<E>,
    {
        let lines = self.lines_of(read);
        let (first, last) = (*lines.start(), *lines.end());
        lines
            .map(|line| {
                let start = if line == first {
                    self.place(read.start)
                } else {
                    self.ranges[line].start
                };
                let end = if line == last {
                    self.place(read.end)
                } else {
                    self.end(line)
                };
                let continues = match (line > first, line < last) {
                    (false, false) => Continues::Neither,
                    (false, true) => Continues::Next,
                    (true, false) => Continues::Previous,
                    (true, true) => Continues::Both,
                };
                (line, build(continues, start..end))
            })
            .collect()
    }

    /// The place in the source of `at`, a place in `text`.
    fn place(&self, at: usize) -> usize {
        let line = self.line(at);
        self.ranges[line].start + (at - self.places[line].start)
    }

    /// The pieces of `nodes`, the children of a node read at `read` in
    /// `text`, on each line that node lies on, in order. A node is cut once
    /// its children are, from a stack of the nodes whose children are being
    /// cut, so nesting takes no call depth.
    fn spread<E>(&self, nodes: Vec<Inline<E>>, read: &Range<usize>) -> Vec<Vec<Inline<E>>>
    where
        E: Extension,
    {
        let mut root = self.cutting(nodes, read);
        let mut open: Vec<(Inline<E>, Cutting<E>)> = Vec::new();
        loop {
            let cutting = open.last_mut().map_or(&mut root, |(_, inner)| inner);
            let Some(mut node) = cutting.rest.pop() else {
                let Some((node, done)) = open.pop() else {
                    return root.spread;
                };
                let pieces = self.cut(node, done.spread);
                open.last_mut()
                    .map_or(&mut root, |(_, outer)| outer)
                    .add(pieces);
                continue;
            };
            if let Some(children) = children_mut(&mut node).map(mem::take) {
                let inner = self.cutting(children, node.span());
                open.push((node, inner));
            } else {
                cutting.add(self.cut(node, Vec::new()));
            }
        }
    }

    /// The spaces and tabs between the text of `line` and its line ending,
    /// in the source: none when something else follows them.
    fn trailing(&self, line: usize) -> Range<usize> {
        let end = self.ranges[line].end;
        end..line_end(self.source, end).unwrap_or(end)
    }

    /// `[[target]]` or `[[target|alias]]` starting at `at`, on one line,
    /// before `end`. Neither part holds a bracket, so its `]]` is the first
    /// mark past its `[[`.
    fn wiki(&self, at: usize, end: usize) -> Option<Wiki> {
        self.text[at..end].strip_prefix("[[")?;
        let start = at + 2;
        let close = *self
            .marks
            .get(self.marks.partition_point(|&mark| mark < start))?;
        if close == start || close + 2 > end || !self.text[close..].starts_with("]]") {
            return None;
        }
        let (target, alias) = self.text[start..close]
            .find('|')
            .map_or((start..close, None), |bar| {
                (start..start + bar, Some(start + bar + 1..close))
            });
        Some(Wiki {
            alias: alias.filter(|range| !range.is_empty()),
            close: close + 2,
            target,
        })
    }
}

impl Links {
    /// The places before `at`, the nearest first.
    fn before(&self, at: usize) -> impl Iterator<Item = usize> {
        iter::successors(self.previous[at], |&place| self.previous[place])
    }

    /// Takes the places between `open` and `close` out of the order.
    fn cut(&mut self, open: usize, close: usize) {
        self.next[open] = Some(close);
        self.previous[close] = Some(open);
    }

    /// Puts a new piece, at the next place, between `open` and `close` in
    /// place of the pieces between them, and gives the first of those.
    fn enclose(&mut self, open: usize, close: usize) -> Option<usize> {
        let inside = self.next[open].filter(|&first| first != close);
        if let Some(first) = inside {
            self.previous[first] = None;
            if let Some(last) = self.previous[close] {
                self.next[last] = None;
            }
        }
        let added = self.next.len();
        self.next[open] = Some(added);
        self.previous[close] = Some(added);
        self.next.push(Some(close));
        self.previous.push(Some(open));
        inside
    }

    /// `count` pieces in the order of their places.
    fn new(count: usize) -> Self {
        Self {
            first: (count > 0).then_some(0),
            next: (1..=count)
                .map(|place| (place < count).then_some(place))
                .collect(),
            previous: (0..count).map(|place| place.checked_sub(1)).collect(),
        }
    }

    /// The pieces in order from `start`.
    fn onward(&self, start: Option<usize>) -> impl Iterator<Item = usize> {
        iter::successors(start, |&place| self.next[place])
    }

    /// Takes the piece at `at` out of the order.
    fn remove(&mut self, at: usize) {
        let (before, after) = (self.previous[at], self.next[at]);
        match before {
            Some(earlier) => self.next[earlier] = after,
            None => self.first = after,
        }
        if let Some(later) = after {
            self.previous[later] = before;
        }
    }
}

impl<E> Piece<E>
where
    E: Extension,
{
    /// How deep the piece nests.
    const fn depth(&self) -> usize {
        match self {
            Self::Node(_, depth) | Self::Pair(_, depth, _) => *depth,
            Self::Run(_) => 0,
        }
    }
}

impl Run {
    /// Which openers a closer can pair with: by its character, whether it
    /// can also open, and its length modulo 3.
    fn key(&self) -> usize {
        usize::from(self.character == '_') * 6 + usize::from(self.can_open) * 3 + self.length % 3
    }

    /// Whether this run can open the emphasis `closer` closes. When either
    /// run can both open and close, their lengths may not add up to a
    /// multiple of 3 unless both are multiples of 3.
    const fn opens(&self, closer: &Self) -> bool {
        let both = self.length + closer.length;
        self.can_open
            && self.character == closer.character
            && (!(self.can_close || closer.can_open)
                || !both.is_multiple_of(3)
                || (self.length.is_multiple_of(3) && closer.length.is_multiple_of(3)))
    }

    /// The characters of the run not yet paired, as text.
    fn text<E>(&self) -> Inline<E>
    where
        E: Extension,
    {
        Inline::Text(Text {
            kind: Kind(None),
            span: self.span.clone(),
            value: self.character.to_string().repeat(self.count),
        })
    }
}

impl<'lay, E> Scanner<'lay, E>
where
    E: Extension,
{
    /// An autolink, inline HTML, or a `<` that starts neither.
    fn angle(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        match self.layout.node_at(at, end) {
            Some(Found::Autolink { close, target }) => {
                let shown = at + 1..close - 1;
                let link = Inline::Link(Link {
                    children: vec![Inline::Text(Text {
                        kind: Kind(None),
                        value: source[shown.clone()].to_owned(),
                        span: shown,
                    })],
                    continues: Continues::Neither,
                    form: LinkForm::Markdown,
                    kind: Kind(None),
                    span: at..*close,
                    target: target.clone(),
                    title: None,
                });
                self.push(link, 1);
                *close
            }
            Some(&Found::Html(close)) => {
                let html = Inline::Html(Html {
                    continues: Continues::Neither,
                    kind: Kind(None),
                    span: at..close,
                    value: source[at..close].to_owned(),
                });
                self.push(html, 0);
                close
            }
            Some(
                Found::Code(_)
                | Found::Link {
                    close: _,
                    end: _,
                    target: _,
                },
            )
            | None => {
                self.push_text(at..at + 1, "<");
                at + 1
            }
        }
    }

    /// An image, or a `!` that starts none.
    fn bang(&mut self, at: usize, end: usize) -> usize {
        if let Some((image, close)) = image_at(self.layout, at, end) {
            self.push(Inline::Image(image), 0);
            return close;
        }
        self.push_text(at..at + 1, "!");
        at + 1
    }

    /// A link, or a `[` that starts none.
    fn bracket(&mut self, at: usize, end: usize) -> usize {
        if let Some(close) = self.link(at, end) {
            return close;
        }
        self.push_text(at..at + 1, "[");
        at + 1
    }

    /// A code span closes at the next run of exactly as many backticks.
    fn code(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        let ticks = run(source.as_bytes(), at, end);
        let Some(&Found::Code(close)) = self.layout.node_at(at, end) else {
            self.push_text(at..at + ticks, &source[at..at + ticks]);
            return at + ticks;
        };
        let code = Inline::Code(Code {
            continues: Continues::Neither,
            kind: Kind(None),
            span: at..close,
            value: unpad(&source[at + ticks..close - ticks]).to_owned(),
        });
        self.push(code, 0);
        close
    }

    /// A run of `*` or `_`, which can open emphasis when it is
    /// left-flanking and close it when it is right-flanking, by the
    /// characters around it. A `_` inside a word does neither.
    fn delimiters(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        let character = char::from(source.as_bytes()[at]);
        let length = source[at..end]
            .chars()
            .take_while(|&next| next == character)
            .count();
        let close = at + length;
        let before = source[..at].chars().next_back();
        let after = source[close..].chars().next();
        let space_before = before.is_none_or(is_whitespace);
        let space_after = after.is_none_or(is_whitespace);
        let mark_before = before.is_some_and(is_punctuation);
        let mark_after = after.is_some_and(is_punctuation);
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
        self.flush();
        let place = self.pieces.len();
        self.pieces.push(Piece::Run(self.runs.len()));
        self.runs.push(Run {
            can_close,
            can_open,
            character,
            count: length,
            deepest: 0,
            length,
            place,
            span: at..close,
        });
        close
    }

    /// The end of a line: the text read so far ends, and two or more spaces
    /// that end the line are a hard line break, with an empty span at the
    /// line end.
    fn end_line(&mut self, at: usize) -> usize {
        if matches!(self.layout.ending(at), Ending::Spaces(_)) {
            let line_break = Inline::LineBreak(LineBreak {
                kind: Kind(None),
                span: at..at,
            });
            self.push(line_break, 0);
        } else {
            self.flush();
        }
        at + 1
    }

    /// An entity or numeric character reference, as the characters it
    /// names, or a `&` that starts none.
    fn entity(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        if let Some((characters, length)) = entity::reference(&source[at..end]) {
            self.push_text(at..at + length, &characters);
            return at + length;
        }
        self.push_text(at..at + 1, "&");
        at + 1
    }

    /// A backslash before ASCII punctuation makes that character plain text.
    /// A backslash that ends a line followed by another is a hard line break.
    fn escape(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        let bytes = source.as_bytes();
        if bytes.get(at + 1) == Some(&b'\n') && matches!(self.layout.ending(at), Ending::Backslash)
        {
            let line_break = Inline::LineBreak(LineBreak {
                kind: Kind(None),
                span: at..at + 1,
            });
            self.push(line_break, 0);
            return at + 1;
        }
        let escaped = at + 1 < end && bytes[at + 1].is_ascii_punctuation();
        let (from, to) = if escaped {
            (at + 1, at + 2)
        } else {
            (at, at + 1)
        };
        self.push_text(at..to, &source[from..to]);
        to
    }

    /// The nodes read, with their runs paired into emphasis, and how deep
    /// the deepest of them nests.
    fn finish(mut self) -> (Vec<Inline<E>>, usize) {
        self.flush();
        emphasize(self.pieces, self.runs)
    }

    fn flush(&mut self) {
        if let Some(start) = self.text_start.take() {
            let text = Inline::Text(Text {
                kind: Kind(None),
                span: start..self.text_end,
                value: mem::take(&mut self.text),
            });
            self.pieces.push(Piece::Node(text, 0));
        }
    }

    /// A wiki link, or a link the bracket pass read: `[text](destination
    /// "title")` or a reference link. A link that would nest deeper than
    /// `INLINE_CAP` is none.
    fn link(&mut self, at: usize, end: usize) -> Option<usize> {
        let source = self.layout.text.as_str();
        if let Some(wiki) = self.layout.wiki(at, end) {
            let children = wiki.alias.map_or_else(Vec::new, |alias| {
                vec![Inline::Text(Text {
                    kind: Kind(None),
                    value: source[alias.clone()].to_owned(),
                    span: alias,
                })]
            });
            let link = Inline::Link(Link {
                children,
                continues: Continues::Neither,
                form: LinkForm::Wiki,
                kind: Kind(None),
                span: at..wiki.close,
                target: source[wiki.target].to_owned(),
                title: None,
            });
            self.push(link, 1);
            return Some(wiki.close);
        }
        let Some(Found::Link {
            close: text_close,
            end: close,
            target,
        }) = self.layout.node_at(at, end)
        else {
            return None;
        };
        let mut inner = Self::new(self.layout);
        inner.scan(at + 1..*text_close);
        let (children, depth) = inner.finish();
        if depth >= INLINE_CAP {
            return None;
        }
        let link = Inline::Link(Link {
            children,
            continues: Continues::Neither,
            form: LinkForm::Markdown,
            kind: Kind(None),
            span: at..*close,
            target: target.destination.clone(),
            title: target.title.clone(),
        });
        self.push(link, depth + 1);
        Some(*close)
    }

    const fn new(layout: &'lay Layout<'lay>) -> Self {
        Self {
            layout,
            pieces: Vec::new(),
            runs: Vec::new(),
            text: String::new(),
            text_end: 0,
            text_start: None,
        }
    }

    /// Text up to the next character that can start another node, or the
    /// end of the text of its line. The spaces and tabs between that text
    /// and the line ending are no text, as a soft line break drops them.
    fn plain(&mut self, at: usize, end: usize) -> usize {
        let source = self.layout.text.as_str();
        let bytes = source.as_bytes();
        let (written, close) = self.layout.line_ends(at);
        if at >= written {
            return close.min(end);
        }
        let stop = written.min(end);
        let mut next = at + 1;
        while next < stop
            && !matches!(
                bytes[next],
                b'\n' | b'\\' | b'`' | b'[' | b'!' | b'&' | b'<' | b'*' | b'_'
            )
        {
            next += 1;
        }
        self.push_text(at..next, &source[at..next]);
        next
    }

    /// Ends the text read so far and adds `node`, which nests `depth` deep,
    /// after it.
    fn push(&mut self, node: Inline<E>, depth: usize) {
        self.flush();
        if let Some(run) = self.runs.last_mut() {
            run.deepest = run.deepest.max(depth);
        }
        self.pieces.push(Piece::Node(node, depth));
    }

    fn push_text(&mut self, range: Range<usize>, value: &str) {
        if self.text_start.is_none() {
            self.text_start = Some(range.start);
        }
        self.text_end = range.end;
        self.text.push_str(value);
    }

    fn scan(&mut self, range: Range<usize>) {
        let bytes = self.layout.text.as_bytes();
        let mut at = range.start;
        while at < range.end {
            at = match bytes[at] {
                b'\n' => self.end_line(at),
                b'\\' => self.escape(at, range.end),
                b'`' => self.code(at, range.end),
                b'!' => self.bang(at, range.end),
                b'&' => self.entity(at, range.end),
                b'<' => self.angle(at, range.end),
                b'*' | b'_' => self.delimiters(at, range.end),
                b'[' => self.bracket(at, range.end),
                _ => self.plain(at, range.end),
            };
        }
    }
}

/// The lines of a paragraph and the inline nodes written on them, with the
/// link `definitions` of the file. Nodes are matched over the whole
/// paragraph, and a node that crosses a line end is cut into one piece per
/// line.
pub fn lines<E>(source: &str, ranges: &[Range<usize>], definitions: &Definitions) -> Vec<Line<E>>
where
    E: Extension,
{
    let mut layout = Layout::new(source, ranges, definitions);
    let mut scanner = Scanner::new(&layout);
    scanner.scan(0..layout.text.len());
    let (nodes, _) = scanner.finish();
    layout.hold(&nodes);
    layout.lines(nodes)
}

/// Pairs the runs among `pieces` into emphasis and strong emphasis, by
/// `CommonMark`'s procedure for processing emphasis, and gives the nodes and
/// how deep the deepest of them nests. A pair that would nest deeper than
/// `INLINE_CAP` is not made, as if its closer found no opener.
fn emphasize<E>(mut pieces: Vec<Piece<E>>, mut runs: Vec<Run>) -> (Vec<Inline<E>>, usize)
where
    E: Extension,
{
    let mut links = Links::new(pieces.len());
    // The runs not yet removed, by their places among `runs`.
    let mut chain = Links::new(runs.len());
    // For each key of closer, the first run that may still open for it.
    let mut bottoms = [0_usize; 12];
    let mut at = links.first;
    while let Some(current) = at {
        let closer = match pieces[current] {
            Piece::Run(closer) if runs[closer].can_close => closer,
            Piece::Node(_, _) | Piece::Pair(_, _, _) | Piece::Run(_) => {
                at = links.next[current];
                continue;
            }
        };
        let key = runs[closer].key();
        let mut inside = 0;
        let found = chain
            .before(closer)
            .take_while(|&run| run >= bottoms[key])
            .find(|&run| {
                inside = inside.max(runs[run].deepest);
                runs[run].opens(&runs[closer])
            });
        let depth = inside + 1;
        if let Some(opener) = found.filter(|_| depth <= INLINE_CAP) {
            at = pair(
                &mut pieces,
                &mut links,
                &mut chain,
                &mut runs,
                (opener, closer),
                depth,
            );
        } else {
            bottoms[key] = closer;
            if !runs[closer].can_open {
                pieces[current] = Piece::Node(runs[closer].text(), 0);
                retire(&mut chain, &mut runs, closer);
            }
            at = links.next[current];
        }
    }
    let depth = deepest(&pieces, links.onward(links.first));
    (nodes(pieces, &links, &runs), depth)
}

/// How deep the deepest of `pieces` at `places` nests.
fn deepest<E, I>(pieces: &[Piece<E>], places: I) -> usize
where
    E: Extension,
    I: IntoIterator<Item = usize>,
{
    places
        .into_iter()
        .map(|place| pieces[place].depth())
        .max()
        .unwrap_or_default()
}

/// Makes emphasis, or strong emphasis when both runs have two characters
/// left, nesting `depth` deep, of the pieces between the opening and the
/// closing run, each given by its place among `runs`. The runs between them
/// leave `chain`, the runs not yet removed. Gives where the closer, or what
/// follows it, now is among the pieces.
fn pair<E>(
    pieces: &mut Vec<Piece<E>>,
    links: &mut Links,
    chain: &mut Links,
    runs: &mut [Run],
    (opener, closer): (usize, usize),
    depth: usize,
) -> Option<usize>
where
    E: Extension,
{
    let (open, close) = (runs[opener].place, runs[closer].place);
    let used = if runs[opener].count >= 2 && runs[closer].count >= 2 {
        2
    } else {
        1
    };
    runs[opener].count -= used;
    runs[opener].span.end -= used;
    runs[closer].count -= used;
    runs[closer].span.start += used;
    let span = runs[opener].span.end..runs[closer].span.start;
    let node = if used == 2 {
        Inline::Strong(Strong {
            children: Vec::new(),
            continues: Continues::Neither,
            kind: Kind(None),
            span,
        })
    } else {
        Inline::Emphasis(Emphasis {
            children: Vec::new(),
            continues: Continues::Neither,
            kind: Kind(None),
            span,
        })
    };
    let first = links.enclose(open, close);
    pieces.push(Piece::Pair(node, depth, first));
    chain.cut(opener, closer);
    runs[opener].deepest = depth;
    if runs[opener].count == 0 {
        links.remove(open);
        retire(chain, runs, opener);
    }
    if runs[closer].count == 0 {
        let after = links.next[close];
        links.remove(close);
        retire(chain, runs, closer);
        return after;
    }
    Some(close)
}

/// Takes `run` out of `chain`, the runs not yet removed: the pieces after
/// it now follow the run before it.
fn retire(chain: &mut Links, runs: &mut [Run], run: usize) {
    if let Some(earlier) = chain.previous[run] {
        runs[earlier].deepest = runs[earlier].deepest.max(runs[run].deepest);
    }
    chain.remove(run);
}

/// `pieces` as nodes, in the order of `links`: each run as the text of its
/// characters not paired, each pair with its children, and texts that touch
/// joined into one. A pair waits on a stack while its children are built,
/// so nesting takes no call depth.
fn nodes<E>(pieces: Vec<Piece<E>>, links: &Links, runs: &[Run]) -> Vec<Inline<E>>
where
    E: Extension,
{
    let mut slots: Vec<Option<Piece<E>>> = pieces.into_iter().map(Some).collect();
    // Each pair waiting for its children, with its siblings built before it
    // and the place of the piece after it.
    let mut waiting = Vec::new();
    let mut built = Vec::new();
    let mut at = links.first;
    loop {
        if let Some(place) = at {
            at = links.next[place];
            match slots[place].take() {
                Some(Piece::Node(node, _)) => join(&mut built, node),
                Some(Piece::Pair(node, _, first)) => {
                    waiting.push((node, mem::take(&mut built), at));
                    at = first;
                }
                Some(Piece::Run(run)) => join(&mut built, runs[run].text()),
                None => {}
            }
        } else if let Some((mut node, siblings, after)) = waiting.pop() {
            let children = mem::replace(&mut built, siblings);
            if let Some(slot) = children_mut(&mut node) {
                *slot = children;
            }
            built.push(node);
            at = after;
        } else {
            return built;
        }
    }
}

/// Adds `node` after `nodes`, joined into the last of them when both are
/// texts that touch. A line end lies between the texts of two lines.
fn join<E>(nodes: &mut Vec<Inline<E>>, node: Inline<E>)
where
    E: Extension,
{
    match (nodes.last_mut(), node) {
        (Some(Inline::Text(last)), Inline::Text(text)) if last.span.end == text.span.start => {
            last.value.push_str(&text.value);
            last.span.end = text.span.end;
        }
        (_, other) => nodes.push(other),
    }
}

/// The children of an emphasis, a link or a strong emphasis.
pub const fn children_mut<E>(node: &mut Inline<E>) -> Option<&mut Vec<Inline<E>>>
where
    E: Extension,
{
    match node {
        Inline::Emphasis(emphasis) => Some(&mut emphasis.children),
        Inline::Link(link) => Some(&mut link.children),
        Inline::Strong(strong) => Some(&mut strong.children),
        Inline::Code(_)
        | Inline::Html(_)
        | Inline::Image(_)
        | Inline::LineBreak(_)
        | Inline::Text(_) => None,
    }
}

/// A Unicode whitespace character, as `CommonMark` counts them: a space
/// separator, a tab, a line feed, a form feed or a carriage return.
pub fn is_whitespace(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{c}' | '\r')
        || character.general_category() == GeneralCategory::SpaceSeparator
}

/// A Unicode punctuation character, as `CommonMark` counts them: one of the
/// punctuation or symbol general categories.
pub fn is_punctuation(character: char) -> bool {
    matches!(
        character.general_category_group(),
        GeneralCategoryGroup::Punctuation | GeneralCategoryGroup::Symbol
    )
}

/// The target of the autolink at `at`, `<scheme:rest>` or `<address>`, and
/// where it ends, past its `>`. Neither holds a space, a control character
/// or a `<`, so the search for the `>` stops at the first of them.
fn autolink_at(source: &str, at: usize, end: usize) -> Option<(String, usize)> {
    let rest = &source[at + 1..end];
    let stop = rest.find(|character: char| {
        character.is_ascii_control() || matches!(character, ' ' | '<' | '>')
    })?;
    let inner = rest[stop..].starts_with('>').then_some(&rest[..stop])?;
    let close = at + inner.len() + 2;
    if is_uri(inner) {
        return Some((inner.to_owned(), close));
    }
    is_email(inner).then(|| (format!("mailto:{inner}"), close))
}

/// A scheme of 2 to 32 letters, digits, `+`, `.` or `-` that starts with a
/// letter, then `:`, then no space, control character or `<`.
fn is_uri(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    (2..=32).contains(&scheme.len())
        && scheme.starts_with(|letter: char| letter.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))
        && !rest
            .chars()
            .any(|character| character.is_ascii_control() || matches!(character, ' ' | '<'))
}

/// An email address as `CommonMark` reads one in an autolink.
fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&byte))
        && domain.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// An image starting at the `!` at `at`, and where it ends: a wiki embed,
/// `![alt](destination "title")`, or a reference image, which takes the
/// target of the definition its label names.
fn image_at<E>(layout: &Layout<'_>, at: usize, end: usize) -> Option<(Image<E>, usize)>
where
    E: Extension,
{
    let source = layout.text.as_str();
    let open = at + 1;
    if let Some(wiki) = layout.wiki(open, end) {
        let image = Image {
            alt: wiki
                .alias
                .map_or_else(String::new, |alias| source[alias].to_owned()),
            continues: Continues::Neither,
            form: LinkForm::Wiki,
            kind: Kind(None),
            span: at..wiki.close,
            target: source[wiki.target].to_owned(),
            title: None,
        };
        return Some((image, wiki.close));
    }
    let Some(Found::Link {
        close: text_close,
        end: close,
        target,
    }) = layout.node_at(open, end)
    else {
        return None;
    };
    let image = Image {
        alt: unspaced(&source[open + 1..*text_close]),
        continues: Continues::Neither,
        form: LinkForm::Markdown,
        kind: Kind(None),
        span: at..*close,
        target: target.destination.clone(),
        title: target.title.clone(),
    };
    Some((image, *close))
}

/// The target of the link or image whose text runs from the `[` at `open`
/// to the `]` at `close` in `source`, read up to `end`, and where it ends:
/// an inline destination and title, or a reference.
fn link_target(
    source: &str,
    definitions: &Definitions,
    open: usize,
    close: usize,
    end: usize,
) -> Option<(Target, usize)> {
    definition::inline(&source[..end], close + 1)
        .or_else(|| reference(source, definitions, open, close, end))
}

/// The target of the reference link or image whose text runs from the `[`
/// at `open` to the `]` at `close` in `source`, read up to `end`, and where
/// it ends: `[text][label]`, `[label][]` or `[label]`. A label with no
/// definition gives none.
fn reference(
    source: &str,
    definitions: &Definitions,
    open: usize,
    close: usize,
    end: usize,
) -> Option<(Target, usize)> {
    let text = &source[..end];
    let named = |label: &str| definitions.get(&fold_label(label)).cloned();
    let after = close + 1;
    if let Some(label_close) = definition::label_at(text, after) {
        let target = named(&text[after + 1..label_close])?;
        return Some((target, label_close + 1));
    }
    let label_close = definition::label_at(text, open).filter(|&found| found == close)?;
    let target = named(&text[open + 1..label_close])?;
    let link_end = if text[after..].starts_with("[]") {
        after + 2
    } else {
        after
    };
    Some((target, link_end))
}

/// The content of a code span without one space at each end, when both
/// ends are spaces and it is not all spaces, a line ending counting as one.
/// A line ending at an end stays, as the boundary between the pieces of the
/// code span.
fn unpad(content: &str) -> &str {
    let space = |byte: &u8| matches!(byte, b' ' | b'\n');
    let bytes = content.as_bytes();
    if !bytes.first().is_some_and(space)
        || !bytes.last().is_some_and(space)
        || bytes.iter().all(space)
    {
        return content;
    }
    let front = content.strip_prefix(' ').unwrap_or(content);
    front.strip_suffix(' ').unwrap_or(front)
}

/// The value of the code span at `span` of `source`, written on `ranges`,
/// the lines of a heading, which reads it whole: each line ending is a
/// space before the strip.
pub fn heading_code(source: &str, ranges: &[Range<usize>], span: &Range<usize>) -> String {
    let first = ranges
        .partition_point(|range| range.start <= span.start)
        .saturating_sub(1);
    let written = (first..ranges.len())
        .map(|index| extent(source, ranges, index))
        .take_while(|line| line.start < span.end)
        .map(|line| &source[line.start.max(span.start)..line.end.min(span.end)])
        .collect::<Vec<_>>()
        .join("\n");
    let ticks = run(written.as_bytes(), 0, written.len());
    unpad(&written[ticks..written.len() - ticks].replace('\n', " ")).to_owned()
}

/// Where line `index` of `ranges` is read in `source`: its text, with the
/// spaces and tabs before its line ending when another line follows it.
fn extent(source: &str, ranges: &[Range<usize>], index: usize) -> Range<usize> {
    let range = &ranges[index];
    let end = if index + 1 < ranges.len() {
        line_end(source, range.end).unwrap_or(range.end)
    } else {
        range.end
    };
    range.start..end
}

/// `text` without the spaces and tabs before each line ending in it.
fn unspaced(text: &str) -> String {
    let mut value = String::with_capacity(text.len());
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            value.truncate(value.trim_end_matches([' ', '\t']).len());
            value.push('\n');
        }
        value.push_str(part);
    }
    value
}

/// Past the spaces and tabs at `end` in `source`, when a line ending, or
/// the end of `source`, follows them.
fn line_end(source: &str, end: usize) -> Option<usize> {
    let close = end
        + source[end..]
            .bytes()
            .take_while(|&byte| matches!(byte, b' ' | b'\t'))
            .count();
    matches!(source.as_bytes().get(close), None | Some(b'\n' | b'\r')).then_some(close)
}

/// Keeps `node`, read at `at`, among `found`, and gives where it ends.
fn keep(found: &mut HashMap<usize, Found>, at: usize, node: Found) -> usize {
    let end = node.end();
    found.insert(at, node);
    end
}

/// Where each run of backticks in `text` starts, by its length.
fn backtick_runs(text: &str) -> HashMap<usize, Vec<usize>> {
    let bytes = text.as_bytes();
    let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut at = 0;
    while let Some(offset) = text[at..].find('`') {
        let start = at + offset;
        let length = run(bytes, start, bytes.len());
        runs.entry(length).or_default().push(start);
        at = start + length;
    }
    runs
}

/// The length of the run of backticks starting at `at`.
fn run(bytes: &[u8], at: usize, end: usize) -> usize {
    bytes[at..end]
        .iter()
        .take_while(|&&byte| byte == b'`')
        .count()
}
