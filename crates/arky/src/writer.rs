//! A document tree written back to markdown, in arky's own form: one
//! written form for each construct, whatever form the file used.
//!
//! [`write()`] writes a tree, parsed or built, with the [`WriteOptions`] that pick the
//! frontmatter's syntax. A [`Writer`] is what an extension writes its own nodes with. The
//! written text is not the bytes that were read: headings get `#` marks, bullets `-`, emphasis
//! `*`, and so on, except where that form would read back as something else. A text written once is written the same way again, so files kept in arky's
//! form do not change from one save to the next.
//!
//! ```rust
//! use arky::parser::parse;
//! use arky::writer::{WriteOptions, write};
//!
//! let source = concat!(
//!     "Notes\n=====\n\nSome __bold__ and _em_ text.\n\n",
//!     "* one\n*   two\n    continued\n\n~~~python\nprint(1)\n~~~\n\n___\n",
//! );
//! let options = WriteOptions::default();
//! let text = write(&parse(source).document, &options)?;
//! assert_eq!(
//!     text,
//!     concat!(
//!         "# Notes\n\nSome **bold** and *em* text.\n\n",
//!         "- one\n- two\n  continued\n\n```python\nprint(1)\n```\n\n***\n",
//!     ),
//! );
//! assert_eq!(write(&parse(&text).document, &options)?, text);
//! # Ok::<(), arky::writer::WriteError>(())
//! ```
//!
//! [`GRAMMAR.md`](https://github.com/tixena/arky/blob/master/crates/arky/GRAMMAR.md#writing)
//! lists the form each construct is written in.

#[cfg(feature = "binary-format")]
mod archived;
mod inline;
mod table;
mod toml;
mod yaml;

use core::fmt::{self, Write as _};
use core::{iter, mem};

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use thiserror::Error;

#[cfg(feature = "binary-format")]
use crate::parser::ast::ArchivedRawDocument;
use crate::parser::ast::{
    Args, Block, CodeBlock, FrontmatterFormat, Heading, Kind, Line, List, Mapping, MarkdownBlock,
    Paragraph, RawDocument, Section, Value,
};
use crate::parser::extension::{Extension, Written};
use crate::parser::frontmatter;
use crate::writer::inline::{Near, Pieces, Segments};

/// The deepest heading level markdown has. A section deeper than this is
/// written inside its parent as a `markdown` block.
const DEEPEST: usize = 6;

/// What is written before a writer's text in its block: nothing, or text,
/// as when an extension writes a node it claimed after other text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Before {
    Nothing,
    Text,
}

/// The marker of a list's items: `-` for a bullet list and `.` after an
/// ordered list's numbers, or the other one, `*` or `)`, for a list right
/// after a list of its kind written with `-` or `.`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Marker {
    First,
    Other,
}

/// The place of a section in its list of sections: the first of a
/// markdown block, the first subsection of its parent, or after a sibling,
/// which a heading line, empty when it has no heading, keeps it apart from.
#[derive(Clone, Copy, Default)]
enum Place {
    #[default]
    BlockFirst,
    Later,
    SubsectionFirst,
}

/// A frontmatter value that holds no other value, as the YAML and TOML
/// writers take it from either tree.
#[derive(Clone, Copy)]
enum Scalar<'value> {
    Boolean(bool),
    Date(NaiveDate),
    DateTime(NaiveDateTime),
    Float(f64),
    Integer(i64),
    Null,
    OffsetDateTime(DateTime<FixedOffset>),
    String(&'value str),
    Time(NaiveTime),
}

/// A frontmatter value as the YAML and TOML writers walk it: a mapping, a
/// list, or a scalar.
enum Shape<'value, M, S> {
    Mapping(&'value M),
    Scalar(Scalar<'value>),
    Sequence(&'value [S]),
}

impl Place {
    /// The place of the section at `index` in a markdown block's list.
    const fn in_block(index: usize) -> Self {
        if index == 0 {
            Self::BlockFirst
        } else {
            Self::Later
        }
    }

    /// The place of the subsection at `index` in its parent's list.
    const fn in_parent(index: usize) -> Self {
        if index == 0 {
            Self::SubsectionFirst
        } else {
            Self::Later
        }
    }
}

/// Why a document could not be written: a value its syntax cannot hold, such as a null in TOML,
/// or an invalid frontmatter whose text holds a line that would close it.
///
/// It displays as its message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
#[non_exhaustive]
pub struct WriteError {
    /// The path of the frontmatter or `Toml` args key whose value cannot be written, such as
    /// `owner.email`, or empty for an invalid frontmatter's closing line.
    pub key: String,
    /// What cannot be written, such as `TOML cannot hold a null`.
    pub message: String,
}

/// How a document is written.
///
/// The struct is `#[non_exhaustive]`: start from `WriteOptions::default()`, then set its field.
///
/// ```rust
/// use arky::parser::ast::FrontmatterFormat;
/// use arky::writer::WriteOptions;
///
/// let mut options = WriteOptions::default();
/// options.frontmatter = FrontmatterFormat::Toml;
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct WriteOptions {
    /// The syntax a valid document's frontmatter is written in, YAML by default, whatever syntax
    /// the file was read in.
    ///
    /// A frontmatter no entry of reads is written back as it was read, in its own syntax.
    pub frontmatter: FrontmatterFormat,
}

/// Builds the markdown text. Extensions write their nodes through it.
///
/// It has no public constructor: [`write()`] makes one and hands it to
/// [`Extension::write`] for each node an extension claimed.
pub struct Writer {
    /// What is written before this writer's text in its block.
    after_text: Before,
    /// Whether a blank line is due before the next text written.
    blank: bool,
    /// The depth of the section being written, from 1.
    depth: usize,
    /// The first error met while writing, which `write` returns.
    error: Option<WriteError>,
    /// Whether the text written starts the file, with no frontmatter before
    /// it.
    file_start: bool,
    /// The syntax a valid document's frontmatter is written in.
    frontmatter: FrontmatterFormat,
    /// The marker the list written next takes.
    marker: Marker,
    out: String,
    /// The place of the section written next in its list.
    place: Place,
    /// Whether blocks follow each other with no blank line, as in the items
    /// of a tight list.
    tight: bool,
}

impl Writer {
    /// Whether the next text written is the first of the file.
    const fn at_file_start(&self) -> bool {
        self.file_start && self.out.is_empty()
    }

    fn block<T>(&mut self, block: &Block<T>)
    where
        T: Extension,
    {
        match block {
            Block::BlockQuote(quote) => self.node(&quote.kind, Written::BlockQuote(quote)),
            Block::CodeBlock(code) => self.node(&code.kind, Written::CodeBlock(code)),
            Block::Html(html) => self.node(&html.kind, Written::Html(html)),
            Block::List(list) => self.node(&list.kind, Written::List(list)),
            Block::Paragraph(paragraph) => {
                self.node(&paragraph.kind, Written::Paragraph(paragraph));
            }
            Block::Table(table) => self.node(&table.kind, Written::Table(table)),
            Block::ThematicBreak(thematic_break) => {
                self.node(&thematic_break.kind, Written::ThematicBreak(thematic_break));
            }
        }
    }

    /// `> ` before each line of the quoted text, `>` alone on an empty one.
    fn block_quote(&mut self, value: &str) {
        for line in value.split('\n') {
            if line.is_empty() {
                self.put(">\n");
            } else {
                self.put("> ");
                self.put(line);
                self.put("\n");
            }
        }
    }

    /// What `T` writes for `node`, which it claimed in place of `plain`.
    fn claimed<T>(&mut self, node: &T::Node<T>, plain: Written<'_, T>) -> String
    where
        T: Extension,
    {
        let mut writer = Self {
            after_text: if self.after_text == Before::Text || !self.out.is_empty() {
                Before::Text
            } else {
                Before::Nothing
            },
            depth: self.depth,
            file_start: self.at_file_start(),
            place: mem::take(&mut self.place),
            tight: self.tight,
            ..self.inner()
        };
        T::write(node, plain, &mut writer);
        self.keep_error(&mut writer);
        writer.out
    }

    /// The lines of a code block after `info`, fenced, also when they were
    /// indented.
    fn code_block(&mut self, info: &str, value: &str) {
        self.fenced(info, |inner| {
            if !value.is_empty() {
                inner.put(value);
                inner.put("\n");
            }
        });
    }

    /// Writes `construct` as arky writes it when no extension claims it.
    /// Its children are written as usual, each by the extension that
    /// claimed it, if one did.
    pub fn construct<T>(&mut self, construct: Written<'_, T>)
    where
        T: Extension,
    {
        construct.write_plain(self);
    }

    /// Ends the line written last, if it is open.
    fn end_line(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    /// Writes a fenced block: `info` after a fence of backticks, or of tildes
    /// when `info` holds a backtick, one longer than the longest run of its
    /// mark in the content, and at least three, then the lines `content`
    /// writes, then the closing fence. The content is written with its
    /// levels counted from one.
    pub fn fenced<F>(&mut self, info: &str, content: F)
    where
        F: FnOnce(&mut Self),
    {
        let mut inner = self.inner();
        content(&mut inner);
        self.keep_error(&mut inner);
        let text = inner.finish();
        let mark = if info.contains('`') { '~' } else { '`' };
        let fence: String = iter::repeat_n(mark, longest_run(&text, mark).max(2) + 1).collect();
        self.end_line();
        self.put(&fence);
        self.put(info);
        self.put("\n");
        self.put(&text);
        self.put(&fence);
        self.put("\n");
    }

    /// Writes a file: `frontmatter`, or the error that fails it, then the
    /// body that `body` writes, at the start of the file when `file_start`.
    fn file<F>(&mut self, frontmatter: Result<String, WriteError>, file_start: bool, body: F)
    where
        F: FnOnce(&mut Self),
    {
        let mut text = match frontmatter {
            Ok(text) => text,
            Err(error) => {
                self.record(Some(error));
                return;
            }
        };
        let mut writer = Self {
            file_start,
            ..self.inner()
        };
        body(&mut writer);
        self.keep_error(&mut writer);
        let written = writer.finish();
        // The reader skips a byte-order mark that starts the file.
        match written.strip_prefix('\u{feff}') {
            Some(rest) if text.is_empty() => {
                text.push_str("&#65279;");
                text.push_str(rest);
            }
            Some(_) | None => text.push_str(&written),
        }
        self.put(&text);
    }

    /// The text written, ending with one line ending unless it is empty.
    fn finish(mut self) -> String {
        self.end_line();
        self.out
    }

    fn heading<T>(&mut self, heading: &Heading<T>)
    where
        T: Extension,
    {
        self.heading_line(
            &Pieces::of(&heading.children),
            inline::holds_line_break(&heading.children),
            heading.children.is_empty(),
            |segments, writer| segments.nodes(writer, &heading.children, false),
        );
    }

    /// `#` marks, then the text, a line break as `&#10;` and a backslash before a final `#` run
    /// that would read as the closing sequence. At depth 1 or 2, a line break splits it over
    /// guarded lines, then `===` or `---`, unless it follows text in a tight list.
    fn heading_line<F>(&mut self, pieces: &Pieces, line_break: bool, empty: bool, children: F)
    where
        F: Fn(&mut Segments, &mut Self),
    {
        let underline = match self.depth {
            1 => "===",
            2 => "---",
            _ => "",
        };
        let after_text = self.tight && (self.after_text == Before::Text || !self.out.is_empty());
        if !underline.is_empty() && !after_text && line_break {
            self.put_lines(|writer, pipes| {
                let mut segments = Segments::new(pieces);
                if pipes {
                    segments.escape_pipes();
                }
                segments.cut_at_line_breaks();
                children(&mut segments, writer);
                segments.lines()
            });
            self.put(underline);
        } else {
            self.put(&"#".repeat(self.depth));
            if !empty {
                let mut segments = Segments::new(pieces);
                segments.line_breaks_as_references();
                children(&mut segments, self);
                let mut text = segments.take(Near::Edge, Near::Edge);
                inline::guard_closing_sequence(&mut text);
                self.put(" ");
                self.put(&text);
            }
        }
        self.put("\n");
    }

    /// The info string of `block`: its language, then a space and its args,
    /// unless they are empty. `Toml` args are written as one line of
    /// `key = value` pairs separated by `, `. A pair holding a null, which
    /// TOML cannot hold, is left out, and the error naming its key is kept.
    pub fn info_string<T>(&mut self, block: &CodeBlock<T>) -> String
    where
        T: Extension,
    {
        let language = block.language.as_deref();
        match &block.args {
            Args::Text(text) => info(language, text),
            Args::Toml(mapping) => {
                let (text, error) = toml::args(mapping);
                self.record(error);
                info(language, &text)
            }
        }
    }

    /// Writes inline nodes on their own, with nothing known around them.
    fn inline<F>(&mut self, build: F)
    where
        F: FnOnce(&mut Segments, &mut Self),
    {
        let mut segments = Segments::new(&Pieces::default());
        build(&mut segments, self);
        let text = segments.take(Near::Unknown, Near::Unknown);
        self.put(&text);
    }

    /// A writer for text this one holds, writing a valid document's
    /// frontmatter in the same syntax.
    fn inner(&self) -> Self {
        Self {
            frontmatter: self.frontmatter,
            ..Self::new()
        }
    }

    /// Each of `items` after `-`, or after its number counted up from `start` and `.`, or `*`
    /// and `)` for the other marker; `item` writes an item's lines, those after the first
    /// indented to its text.
    fn items<I, F>(&mut self, items: I, loose: bool, start: Option<u64>, mut item: F)
    where
        I: IntoIterator,
        F: FnMut(&mut Self, I::Item),
    {
        let (bullet, delimiter) = match self.marker {
            Marker::First => ("-", '.'),
            Marker::Other => ("*", ')'),
        };
        for (index, each) in items.into_iter().enumerate() {
            if index > 0 {
                self.separate_items(loose);
            }
            let marker = start.map_or_else(
                || bullet.to_owned(),
                |first| {
                    let number =
                        u64::try_from(index).map_or(first, |step| first.saturating_add(step));
                    format!("{number}{delimiter}")
                },
            );
            let mut inner = Self {
                tight: !loose,
                ..self.inner()
            };
            item(&mut inner, each);
            self.keep_error(&mut inner);
            self.marked(&marker, &inner.finish());
        }
    }

    /// Takes the error that `inner`, a writer this one built, recorded,
    /// unless this one has one already.
    fn keep_error(&mut self, inner: &mut Self) {
        self.record(inner.error.take());
    }

    /// The text of `line`, and whether it starts with text. `first` is set
    /// for the first line of a paragraph.
    fn line<T>(&mut self, line: &Line<T>, segments: &mut Segments, first: bool) -> (String, bool)
    where
        T: Extension,
    {
        if let Some(node) = line.kind.0.as_deref() {
            let text = self.claimed(node, Written::Line(line));
            (text.trim_end_matches('\n').to_owned(), false)
        } else {
            segments.nodes(self, &line.children, false);
            segments.line(first)
        }
    }

    fn list<T>(&mut self, list: &List<T>)
    where
        T: Extension,
    {
        self.items(&list.items, list.loose, list.start, |inner, item| {
            inner.node(&item.kind, Written::Item(item));
        });
    }

    /// Writes `block` with its levels counted from one.
    pub fn markdown_block<T>(&mut self, block: &MarkdownBlock<T>)
    where
        T: Extension,
    {
        let mut inner = Self {
            file_start: self.at_file_start(),
            ..self.inner()
        };
        inner.markdown_node(block);
        self.keep_error(&mut inner);
        self.end_line();
        self.put(&inner.finish());
    }

    fn markdown_node<T>(&mut self, block: &MarkdownBlock<T>)
    where
        T: Extension,
    {
        self.node(&block.kind, Written::MarkdownBlock(block));
    }

    /// Writes `content`, an item's lines, after `marker`, the rest indented to
    /// its text. A first line led by a space or a tab, or making the marker's
    /// line a thematic break, goes under it; after `*`, `***` is written `___`.
    fn marked(&mut self, marker: &str, content: &str) {
        let indent = " ".repeat(marker.len() + 1);
        let mut lines = content.strip_suffix('\n').unwrap_or(content).split('\n');
        self.put(marker);
        if let Some(line) = lines.next().filter(|line| !line.is_empty()) {
            let first = if marker == "*" && line == "***" {
                "___"
            } else {
                line
            };
            if first.starts_with([' ', '\t'])
                || inline::is_thematic_break(&format!("{marker} {first}"))
            {
                self.put("\n");
                self.put(&indent);
            } else {
                self.put(" ");
            }
            self.put(first);
        }
        self.put("\n");
        for line in lines {
            if !line.is_empty() {
                self.put(&indent);
            }
            self.put(line);
            self.put("\n");
        }
    }

    pub(crate) const fn new() -> Self {
        Self {
            after_text: Before::Nothing,
            blank: false,
            depth: 1,
            error: None,
            file_start: false,
            frontmatter: FrontmatterFormat::Yaml,
            marker: Marker::First,
            out: String::new(),
            place: Place::BlockFirst,
            tight: false,
        }
    }

    /// The empty heading line of a section with no heading at `place`, unless a read builds it
    /// again with none: first of a markdown block holding blocks or subsections, or first
    /// subsection of its parent holding subsections and no blocks.
    fn no_heading(&mut self, place: Place, blocks: bool, subsections: bool) {
        match place {
            Place::BlockFirst if blocks || subsections => {}
            Place::SubsectionFirst if !blocks && subsections => {}
            Place::BlockFirst | Place::Later | Place::SubsectionFirst => {
                self.separate();
                self.put(&"#".repeat(self.depth));
                self.put("\n");
            }
        }
    }

    /// Writes a node that stands on its own lines: what the extension that
    /// claimed it writes, or else its plain form.
    fn node<T>(&mut self, kind: &Kind<T>, plain: Written<'_, T>)
    where
        T: Extension,
    {
        self.standalone(|writer| match kind.0.as_deref() {
            Some(node) => {
                let text = writer.claimed(node, plain);
                writer.put(&text);
            }
            None => writer.construct(plain),
        });
    }

    /// Each line of the paragraph on a line of its own, guarded as a
    /// paragraph's lines.
    fn paragraph<T>(&mut self, paragraph: &Paragraph<T>)
    where
        T: Extension,
    {
        let mut pieces = Pieces::default();
        for line in paragraph.lines.iter().filter(|line| line.kind.0.is_none()) {
            pieces.add(&line.children);
        }
        self.put_lines(|writer, pipes| writer.paragraph_lines(paragraph, &pieces, pipes));
    }

    /// The lines of `paragraph` that write something, with a backslash before
    /// every `|` of the text when `pipes` is set, and whether the first
    /// starts with text.
    fn paragraph_lines<T>(
        &mut self,
        paragraph: &Paragraph<T>,
        pieces: &Pieces,
        pipes: bool,
    ) -> (Vec<String>, bool)
    where
        T: Extension,
    {
        let mut segments = Segments::new(pieces);
        if pipes {
            segments.escape_pipes();
        }
        let mut lines = Vec::with_capacity(paragraph.lines.len());
        let mut text_led = false;
        for line in &paragraph.lines {
            let first = lines.is_empty();
            let (text, led) = self.line(line, &mut segments, first);
            if text.is_empty() {
                continue;
            }
            if first {
                text_led = led;
            }
            lines.push(text);
        }
        (lines, text_led)
    }

    /// Picks the marker of the list written next: the other one than that of
    /// the list before it, when that list is of its kind, `after_same_kind`.
    fn pick_marker(&mut self, after_same_kind: bool) {
        self.marker = if after_same_kind && self.marker == Marker::First {
            Marker::Other
        } else {
            Marker::First
        };
    }

    /// The heading or the empty heading line, then the blocks, then the subsections one level
    /// deeper. A plain list right after a plain list of its kind takes the other marker, so the
    /// two read back as two lists.
    fn plain_section<T>(&mut self, section: &Section<T>)
    where
        T: Extension,
    {
        let place = mem::take(&mut self.place);
        match &section.heading {
            Some(heading) => self.node(&heading.kind, Written::Heading(heading)),
            None => self.no_heading(
                place,
                !section.blocks.is_empty(),
                !section.subsections.is_empty(),
            ),
        }
        let mut previous: Option<&List<T>> = None;
        for block in &section.blocks {
            let list = plain_list(block);
            self.pick_marker(
                previous
                    .zip(list)
                    .is_some_and(|(before, after)| before.start.is_some() == after.start.is_some()),
            );
            self.block(block);
            previous = list;
        }
        for (index, subsection) in section.subsections.iter().enumerate() {
            self.subsection(index, |writer| writer.section(subsection));
        }
    }

    /// Adds `text` to what is written, as it is.
    pub fn push_str(&mut self, text: &str) {
        self.put(text);
    }

    /// Adds `text`, after the blank line that is due, if any.
    fn put(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.blank {
            self.out.push('\n');
            self.blank = false;
        }
        self.out.push_str(text);
    }

    /// Writes the lines `build` gives, each on a line of its own, guarded as
    /// a paragraph's lines: a backslash before a first line led by text that
    /// would read as a link reference definition or open a TOML frontmatter,
    /// and the lines built again with every `|` escaped when they would read
    /// as a table. `build` takes whether to escape the `|`s and gives the
    /// lines and whether the first starts with text.
    fn put_lines<F>(&mut self, mut build: F)
    where
        F: FnMut(&mut Self, bool) -> (Vec<String>, bool),
    {
        let file_start = self.at_file_start();
        let guarded = |(mut lines, text_led): (Vec<String>, bool)| {
            if text_led {
                inline::guard_definition(&mut lines);
                if file_start && let Some(first) = lines.first_mut().filter(|first| *first == "+++")
                {
                    first.insert(0, '\\');
                }
            }
            lines
        };
        let mut lines = guarded(build(self, false));
        if inline::reads_as_table(&lines) {
            lines = guarded(build(self, true));
        }
        for line in &lines {
            self.put(line);
            self.put("\n");
        }
    }

    /// Writes `document` as a file holds it: its frontmatter, then its body.
    /// A valid document's frontmatter is written in this writer's syntax, and
    /// an invalid one's as it was read.
    pub fn raw_document<T>(&mut self, document: &RawDocument<T>)
    where
        T: Extension,
    {
        let (frontmatter, file_start) = match document {
            RawDocument::Invalid(invalid) => (
                as_read(&invalid.frontmatter.text, invalid.frontmatter.format),
                false,
            ),
            RawDocument::Valid(valid) => {
                let written = match (&valid.frontmatter, self.frontmatter) {
                    (None, _) => Ok(String::new()),
                    (Some(mapping), FrontmatterFormat::Toml) => toml::frontmatter(mapping),
                    (Some(mapping), FrontmatterFormat::Yaml) => Ok(yaml::frontmatter(mapping)),
                };
                (written, valid.frontmatter.is_none())
            }
        };
        self.file(frontmatter, file_start, |writer| match document {
            RawDocument::Invalid(invalid) => writer.markdown_node(&invalid.body),
            RawDocument::Valid(valid) => writer.node(&valid.kind, Written::Document(valid)),
        });
    }

    /// Keeps `error`, unless an error is kept already.
    fn record(&mut self, error: Option<WriteError>) {
        if self.error.is_none() {
            self.error = error;
        }
    }

    fn section<T>(&mut self, section: &Section<T>)
    where
        T: Extension,
    {
        self.node(&section.kind, Written::Section(section));
    }

    /// Writes the sections of a markdown block, each of which `section`
    /// writes, in a writer of their own.
    fn sections<I, F>(&mut self, sections: I, mut section: F)
    where
        I: IntoIterator,
        F: FnMut(&mut Self, I::Item),
    {
        let mut inner = Self {
            file_start: self.at_file_start(),
            tight: self.tight,
            ..self.inner()
        };
        for (index, each) in sections.into_iter().enumerate() {
            inner.place = Place::in_block(index);
            section(&mut inner, each);
        }
        self.keep_error(&mut inner);
        self.put(&inner.finish());
    }

    /// Ends the line written last, and makes a blank line due before the
    /// next text, outside a tight list.
    fn separate(&mut self) {
        self.end_line();
        if !self.tight && !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.blank = true;
        }
    }

    /// Makes a blank line due between two items of a loose list.
    const fn separate_items(&mut self, loose: bool) {
        if loose {
            self.blank = true;
        }
    }

    /// Writes what `write` writes on lines of its own.
    fn standalone<F>(&mut self, write: F)
    where
        F: FnOnce(&mut Self),
    {
        self.separate();
        write(self);
        self.end_line();
    }

    /// Writes the subsection at `index`, which `section` writes, one level
    /// deeper, or past the deepest level as a `markdown` block.
    fn subsection<F>(&mut self, index: usize, section: F)
    where
        F: FnOnce(&mut Self),
    {
        let depth = self.depth;
        if depth < DEEPEST {
            self.depth = depth + 1;
            self.place = Place::in_parent(index);
            section(self);
            self.depth = depth;
        } else {
            self.separate();
            self.fenced("markdown", section);
        }
    }

    fn thematic_break(&mut self) {
        self.put("***\n");
    }
}

impl<T> Written<'_, T>
where
    T: Extension,
{
    /// Writes the construct as arky writes it when no extension claims
    /// it, with `writer`.
    fn write_plain(self, writer: &mut Writer) {
        match self {
            Self::BlockQuote(quote) => writer.block_quote(&quote.value),
            Self::Code(code) => {
                writer.inline(|segments, _| segments.code(&code.value, code.continues));
            }
            Self::CodeBlock(code) => {
                let info = writer.info_string(code);
                writer.code_block(&info, &code.value);
            }
            Self::Document(document) => writer.markdown_node(&document.body),
            Self::Emphasis(emphasis) => writer.inline(|segments, parent| {
                segments.emphasis(parent, emphasis.continues, &emphasis.children, false);
            }),
            Self::Heading(heading) => writer.heading(heading),
            Self::Html(html) => writer.put(&html.value),
            Self::Image(image) => writer.inline(|segments, _| {
                segments.image(
                    image.form,
                    image.continues,
                    &image.alt,
                    &image.target,
                    image.title.as_deref(),
                );
            }),
            Self::Item(item) => writer.markdown_node(&item.body),
            Self::Line(line) => {
                let mut segments = Segments::new(&Pieces::of(&line.children));
                let (text, _) = writer.line(line, &mut segments, false);
                writer.put(&text);
            }
            Self::LineBreak(_) => writer.put("\\"),
            Self::Link(link) => writer.inline(|segments, parent| segments.link(parent, link)),
            Self::LinkEmphasis(emphasis) => writer.inline(|segments, parent| {
                segments.emphasis(parent, emphasis.continues, &emphasis.children, true);
            }),
            Self::LinkStrong(strong) => writer.inline(|segments, parent| {
                segments.strong(parent, strong.continues, &strong.children, true);
            }),
            Self::List(list) => writer.list(list),
            Self::MarkdownBlock(block) => writer.sections(&block.sections, Writer::section),
            Self::Paragraph(paragraph) => writer.paragraph(paragraph),
            Self::Section(section) => writer.plain_section(section),
            Self::Strong(strong) => writer.inline(|segments, parent| {
                segments.strong(parent, strong.continues, &strong.children, false);
            }),
            Self::Table(table) => table::write(writer, table),
            Self::TableCell(cell) => writer.markdown_node(&cell.body),
            Self::TableRow(row) => {
                let line = table::pipe_row(writer, row);
                writer.put(&line);
            }
            Self::Text(text) => writer.inline(|segments, _| segments.text(&text.value, false)),
            Self::ThematicBreak(_) => writer.thematic_break(),
        }
    }
}

/// Writes `document` in arky's form: its frontmatter, if it has one, in
/// the syntax `options` picks, or as it was read when no entry of it reads,
/// then its body.
///
/// A node an extension claimed is written by that extension.
///
/// # Errors
///
/// Returns an error naming the key when TOML is picked and the frontmatter
/// holds a null, which TOML cannot hold, or when the `Toml` args of a code
/// block hold one, and an error naming the line when the text of an invalid
/// frontmatter holds one that would close it.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::FrontmatterFormat;
/// use arky::parser::parse;
/// use arky::writer::{WriteOptions, write};
///
/// let document = parse("+++\ntitle = \"Notes\"\ndue = 2026-10-10\n+++\n# Notes\n").document;
/// assert_eq!(
///     write(&document, &WriteOptions::default())?,
///     "---\ntitle: Notes\ndue: !!timestamp 2026-10-10\n---\n# Notes\n",
/// );
///
/// let mut toml = WriteOptions::default();
/// toml.frontmatter = FrontmatterFormat::Toml;
/// assert_eq!(
///     write(&document, &toml)?,
///     "+++\ntitle = \"Notes\"\ndue = 2026-10-10\n+++\n# Notes\n",
/// );
///
/// let null = parse("---\nowner:\n  name: Ada\n  email: null\n---\n").document;
/// let error = write(&null, &toml).expect_err("TOML has no null");
/// assert_eq!(error.key, "owner.email");
/// assert_eq!(error.to_string(), "TOML cannot hold a null");
/// # Ok::<(), arky::writer::WriteError>(())
/// ```
pub fn write<E>(document: &RawDocument<E>, options: &WriteOptions) -> Result<String, WriteError>
where
    E: Extension,
{
    let mut writer = Writer {
        frontmatter: options.frontmatter,
        ..Writer::new()
    };
    writer.raw_document(document);
    writer.error.take().map_or(Ok(writer.out), Err)
}

/// Code outside arky writes the binary form's text through
/// [`forms::stored`](crate::forms::stored), which calls this function.
///
/// It writes the archived `document` in place, as [`write()`] writes the same tree: a valid
/// document's frontmatter in the syntax `options` names, YAML by default, and a frontmatter no
/// entry of reads as it was read.
///
/// It exists with the `binary-format` feature. An archived tree holds no extension node, so each
/// node is written in its plain form. Code outside arky has no way to reach an
/// `ArchivedRawDocument` to pass here.
///
/// # Errors
///
/// The errors `write` returns for the same tree, and an error naming the key
/// of a date or time that the archived tree holds out of chrono's range.
#[cfg(feature = "binary-format")]
pub fn write_archived(
    document: &ArchivedRawDocument,
    options: &WriteOptions,
) -> Result<String, WriteError> {
    let mut writer = Writer {
        frontmatter: options.frontmatter,
        ..Writer::new()
    };
    archived::raw_document(&mut writer, document);
    writer.error.take().map_or(Ok(writer.out), Err)
}

/// `text`, an invalid frontmatter's, between the delimiters of `format`, the
/// only syntax the text reads in, or an error naming a line of the text that
/// would close it, as the reader closes a frontmatter.
fn as_read(text: &str, format: FrontmatterFormat) -> Result<String, WriteError> {
    if let Some(line) = frontmatter::closing(text, 0, format) {
        let closing = text[line.start..line.end].trim_end();
        return Err(WriteError {
            key: String::new(),
            message: format!(
                "the invalid frontmatter's text holds the line `{closing}`, which would close it"
            ),
        });
    }
    let delimiter = match format {
        FrontmatterFormat::Toml => "+++",
        FrontmatterFormat::Yaml => "---",
    };
    Ok(format!("{delimiter}\n{text}\n{delimiter}\n"))
}

/// The info string of a code block: `language`, then a space and `args`,
/// unless they are empty.
fn info(language: Option<&str>, args: &str) -> String {
    let mut info = language.unwrap_or_default().to_owned();
    if !args.is_empty() {
        info.push(' ');
        info.push_str(args);
    }
    info
}

/// A date and time with no offset, such as `2026-10-10T07:32:00`, with the
/// fraction of a second when it has one.
fn push_date_time(out: &mut String, date_time: &NaiveDateTime) {
    let _: fmt::Result = write!(out, "{}T{}", date_time.date(), date_time.time());
}

/// A date and time with its offset: `Z` for UTC, such as `+02:00` otherwise.
fn push_offset_date_time(out: &mut String, date_time: &DateTime<FixedOffset>) {
    push_date_time(out, &date_time.naive_local());
    let offset = date_time.offset();
    if offset.local_minus_utc() == 0_i32 {
        out.push('Z');
    } else {
        let _: fmt::Result = write!(out, "{offset}");
    }
}

/// The literal of a float that is not a finite number, of `literals`, its
/// syntax's `nan`, `inf` and `-inf`; `None` for a finite float.
pub(crate) const fn non_finite(float: f64, literals: [&str; 3]) -> Option<&str> {
    let [nan, infinity, negative_infinity] = literals;
    if float.is_nan() {
        Some(nan)
    } else if float.is_finite() {
        None
    } else if float.is_sign_negative() {
        Some(negative_infinity)
    } else {
        Some(infinity)
    }
}

/// A finite float as the shortest text that reads back as it, with `.0`
/// when that text would read as an integer.
fn push_finite(out: &mut String, float: f64) {
    let start = out.len();
    let _: fmt::Result = write!(out, "{float}");
    if !out[start..].contains('.') {
        out.push_str(".0");
    }
}

/// The shape of `value`, as the YAML and TOML writers walk it.
fn shape(value: &Value) -> Shape<'_, Mapping, Value> {
    match value {
        Value::Boolean(boolean) => Shape::Scalar(Scalar::Boolean(*boolean)),
        Value::Date(date) => Shape::Scalar(Scalar::Date(*date)),
        Value::DateTime(date_time) => Shape::Scalar(Scalar::DateTime(*date_time)),
        Value::Float(float) => Shape::Scalar(Scalar::Float(*float)),
        Value::Integer(integer) => Shape::Scalar(Scalar::Integer(*integer)),
        Value::Mapping(mapping) => Shape::Mapping(mapping),
        Value::Null(()) => Shape::Scalar(Scalar::Null),
        Value::OffsetDateTime(date_time) => Shape::Scalar(Scalar::OffsetDateTime(*date_time)),
        Value::Sequence(sequence) => Shape::Sequence(sequence),
        Value::String(text) => Shape::Scalar(Scalar::String(text)),
        Value::Time(time) => Shape::Scalar(Scalar::Time(*time)),
    }
}

/// `block` when it is a list no extension claimed.
const fn plain_list<T>(block: &Block<T>) -> Option<&List<T>>
where
    T: Extension,
{
    match block {
        Block::List(list) if list.kind.0.is_none() => Some(list),
        Block::BlockQuote(_)
        | Block::CodeBlock(_)
        | Block::Html(_)
        | Block::List(_)
        | Block::Paragraph(_)
        | Block::Table(_)
        | Block::ThematicBreak(_) => None,
    }
}

/// The length of the longest run of `mark` in `text`.
fn longest_run(text: &str, mark: char) -> usize {
    text.split(|character| character != mark)
        .map(str::len)
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
