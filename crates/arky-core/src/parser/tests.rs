//! Each sample is a markdown source and the tree it parses into, written as
//! an outline: one node per line, indented under its parent.

pub mod blocks;
#[cfg(feature = "mongodb")]
mod bson;
mod documents;
mod errors;
mod example;
mod extension;
mod frontmatter;
mod inlines;
pub mod json;
pub mod rkyv;
mod spans;
mod tables;
mod toml;
mod unrecognized;
mod yaml;

use core::ops::Range;
use std::thread;

use chrono::{
    DateTime, Datelike as _, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike as _,
};

use crate::parser::ast::{
    Alignment, Args, Block, CodeBlock, Continues, Document, FrontmatterFormat, Inline, Kind,
    LinkForm, LinkInline, Mapping, MarkdownBlock, RawDocument, Section, TableFormat, Value,
};
use crate::parser::extension::{Extension, Markdown};
use crate::parser::line::line_at;
use crate::parser::{ParseOptions, located, parse, parse_located};
use crate::writer::Writer;

/// One node of a tree: how deep it sits, how it reads, what it covers.
/// `merged` marks a pair whose key is written more than once, and `spread` a
/// cell of a table row written over several lines. A node of the model
/// covers nothing: it holds no span.
struct Row {
    depth: usize,
    label: String,
    merged: bool,
    span: Option<Range<usize>>,
    spread: bool,
}

#[derive(Default)]
struct Rows<'src> {
    rows: Vec<Row>,
    /// The text the tree was parsed from, or empty for an outline alone. The
    /// frontmatter is read from it again for the spans of its values.
    source: &'src str,
}

impl Rows<'_> {
    fn block<E>(&mut self, depth: usize, block: &Block<E>)
    where
        E: Extension,
    {
        match block {
            Block::BlockQuote(block_quote) => {
                self.push_unlocated(depth, format!("BlockQuote {:?}", block_quote.value));
            }
            Block::CodeBlock(code_block) => {
                let label = code_block_label(
                    code_block.language.as_deref(),
                    &code_block.args,
                    &code_block.value,
                    code_block.kind.0.is_some(),
                );
                self.push_unlocated(depth, label);
            }
            Block::Html(html) => self.push_unlocated(depth, format!("Html {:?}", html.value)),
            Block::List(list) => {
                self.push_unlocated(depth, list_label(list.start, list.loose));
                for item in &list.items {
                    self.push_unlocated(depth + 1, "Item".to_owned());
                    self.markdown_block(depth + 2, &item.body);
                }
            }
            Block::Paragraph(paragraph) => {
                self.push_unlocated(depth, "Paragraph".to_owned());
                for line in &paragraph.lines {
                    self.push_unlocated(depth + 1, "Line".to_owned());
                    self.inlines(depth + 2, &line.children);
                }
            }
            Block::Table(table) => {
                self.push_unlocated(depth, table_label(table.format, &table.alignments));
                let head = table.head.iter().map(|row| ("TableRow head", row));
                let rows = table.rows.iter().map(|row| ("TableRow", row));
                for (name, row) in head.chain(rows) {
                    self.push_unlocated(depth + 1, name.to_owned());
                    for cell in &row.cells {
                        self.push_unlocated(depth + 2, "TableCell".to_owned());
                        self.markdown_block(depth + 3, &cell.body);
                    }
                }
            }
            Block::ThematicBreak(_) => self.push_unlocated(depth, "ThematicBreak".to_owned()),
        }
    }

    /// A frontmatter is outlined as its reader reads it again from the
    /// source, with the syntax it is written in and each key written more
    /// than once, once that reading is checked to be the model's mapping.
    fn document<E>(&mut self, document: &Document<E>)
    where
        E: Extension,
    {
        self.push_unlocated(0, "Document".to_owned());
        if let Some(mapping) = &document.frontmatter {
            if self.source.is_empty() {
                self.push_unlocated(1, "Frontmatter".to_owned());
                self.mapping(2, mapping);
            } else {
                let frontmatter = located_frontmatter_node(self.source);
                let read = read_again(self.source, &frontmatter);
                assert_eq!(&read.clone().into_model(), mapping);
                let label = format!("Frontmatter format={:?}", frontmatter.format);
                self.push(1, label, &frontmatter.span);
                self.located_mapping(2, &read);
            }
        }
        self.markdown_block(1, &document.body);
    }

    fn inlines<E>(&mut self, depth: usize, inlines: &[Inline<E>])
    where
        E: Extension,
    {
        for inline in inlines {
            match inline {
                Inline::Code(code) => {
                    let label = format!("Code {:?}{}", code.value, continues_label(code.continues));
                    self.push_unlocated(depth, label);
                }
                Inline::Emphasis(emphasis) => {
                    let label = format!("Emphasis{}", continues_label(emphasis.continues));
                    self.push_unlocated(depth, label);
                    self.inlines(depth + 1, &emphasis.children);
                }
                Inline::Html(html) => {
                    let label = format!("Html {:?}{}", html.value, continues_label(html.continues));
                    self.push_unlocated(depth, label);
                }
                Inline::Image(image) => {
                    let label = image_label(
                        image.form,
                        &image.target,
                        image.title.as_deref(),
                        &image.alt,
                        image.continues,
                    );
                    self.push_unlocated(depth, label);
                }
                Inline::LineBreak(_) => self.push_unlocated(depth, "LineBreak".to_owned()),
                Inline::Link(link) => {
                    let label = link_label(
                        link.form,
                        &link.target,
                        link.title.as_deref(),
                        link.continues,
                    );
                    self.push_unlocated(depth, label);
                    self.link_inlines(depth + 1, &link.children);
                }
                Inline::Strong(strong) => {
                    let label = format!("Strong{}", continues_label(strong.continues));
                    self.push_unlocated(depth, label);
                    self.inlines(depth + 1, &strong.children);
                }
                Inline::Text(text) => {
                    self.push_unlocated(depth, format!("Text {:?}", text.value));
                }
            }
        }
    }

    /// A link's text, outlined as `inlines` outlines a line.
    fn link_inlines<E>(&mut self, depth: usize, inlines: &[LinkInline<E>])
    where
        E: Extension,
    {
        for inline in inlines {
            match inline {
                LinkInline::Code(code) => {
                    let label = format!("Code {:?}{}", code.value, continues_label(code.continues));
                    self.push_unlocated(depth, label);
                }
                LinkInline::Emphasis(emphasis) => {
                    let label = format!("Emphasis{}", continues_label(emphasis.continues));
                    self.push_unlocated(depth, label);
                    self.link_inlines(depth + 1, &emphasis.children);
                }
                LinkInline::Html(html) => {
                    let label = format!("Html {:?}{}", html.value, continues_label(html.continues));
                    self.push_unlocated(depth, label);
                }
                LinkInline::Image(image) => {
                    let label = image_label(
                        image.form,
                        &image.target,
                        image.title.as_deref(),
                        &image.alt,
                        image.continues,
                    );
                    self.push_unlocated(depth, label);
                }
                LinkInline::LineBreak(_) => self.push_unlocated(depth, "LineBreak".to_owned()),
                LinkInline::Strong(strong) => {
                    let label = format!("Strong{}", continues_label(strong.continues));
                    self.push_unlocated(depth, label);
                    self.link_inlines(depth + 1, &strong.children);
                }
                LinkInline::Text(text) => {
                    self.push_unlocated(depth, format!("Text {:?}", text.value));
                }
            }
        }
    }

    fn located_block<E>(&mut self, depth: usize, block: &located::Block<E>)
    where
        E: Extension,
    {
        match block {
            located::Block::BlockQuote(block_quote) => {
                let label = format!("BlockQuote {:?}", block_quote.value);
                self.push(depth, label, &block_quote.span);
            }
            located::Block::CodeBlock(code_block) => {
                let label = code_block_label(
                    code_block.language.as_deref(),
                    &code_block.args,
                    &code_block.value,
                    code_block.kind.0.is_some(),
                );
                self.push(depth, label, &code_block.span);
            }
            located::Block::Html(html) => {
                self.push(depth, format!("Html {:?}", html.value), &html.span);
            }
            located::Block::List(list) => {
                self.push(depth, list_label(list.start, list.loose), &list.span);
                for item in &list.items {
                    self.push(depth + 1, "Item".to_owned(), &item.span);
                    self.located_markdown_block(depth + 2, &item.body);
                }
            }
            located::Block::Paragraph(paragraph) => {
                self.push(depth, "Paragraph".to_owned(), &paragraph.span);
                for line in &paragraph.lines {
                    self.push(depth + 1, "Line".to_owned(), &line.span);
                    self.located_inlines(depth + 2, &line.children);
                }
            }
            located::Block::Table(table) => {
                let label = table_label(table.format, &table.alignments);
                self.push(depth, label, &table.span);
                let head = table.head.iter().map(|row| ("TableRow head", row));
                let rows = table.rows.iter().map(|row| ("TableRow", row));
                for (name, row) in head.chain(rows) {
                    self.push(depth + 1, name.to_owned(), &row.span);
                    let spread = self
                        .source
                        .get(row.span.clone())
                        .is_some_and(|text| text.contains('\n'));
                    for cell in &row.cells {
                        self.rows.push(Row {
                            depth: depth + 2,
                            label: "TableCell".to_owned(),
                            merged: false,
                            span: Some(cell.span.clone()),
                            spread,
                        });
                        self.located_markdown_block(depth + 3, &cell.body);
                    }
                }
            }
            located::Block::ThematicBreak(thematic_break) => {
                self.push(depth, "ThematicBreak".to_owned(), &thematic_break.span);
            }
        }
    }

    fn located_document<E>(&mut self, document: &located::Document<E>)
    where
        E: Extension,
    {
        self.push(0, "Document".to_owned(), &document.span);
        match &document.frontmatter {
            Some(Ok(frontmatter)) => {
                let label = format!("Frontmatter format={:?}", frontmatter.format);
                self.push(1, label, &frontmatter.span);
                let read = read_again(self.source, frontmatter);
                assert_eq!(read.clone().into_model(), frontmatter.mapping);
                self.located_mapping(2, &read);
            }
            Some(Err(broken)) => {
                let label = format!(
                    "BrokenFrontmatter format={:?} error={:?}",
                    broken.format, broken.error.message
                );
                self.push(1, label, &broken.span);
            }
            None => {}
        }
        self.located_markdown_block(1, &document.body);
    }

    fn located_inlines<E>(&mut self, depth: usize, inlines: &[located::Inline<E>])
    where
        E: Extension,
    {
        for inline in inlines {
            match inline {
                located::Inline::Code(code) => {
                    let label = format!("Code {:?}{}", code.value, continues_label(code.continues));
                    self.push(depth, label, &code.span);
                }
                located::Inline::Emphasis(emphasis) => {
                    let label = format!("Emphasis{}", continues_label(emphasis.continues));
                    self.push(depth, label, &emphasis.span);
                    self.located_inlines(depth + 1, &emphasis.children);
                }
                located::Inline::Html(html) => {
                    let label = format!("Html {:?}{}", html.value, continues_label(html.continues));
                    self.push(depth, label, &html.span);
                }
                located::Inline::Image(image) => {
                    let label = image_label(
                        image.form,
                        &image.target,
                        image.title.as_deref(),
                        &image.alt,
                        image.continues,
                    );
                    self.push(depth, label, &image.span);
                }
                located::Inline::LineBreak(line_break) => {
                    self.push(depth, "LineBreak".to_owned(), &line_break.span);
                }
                located::Inline::Link(link) => {
                    let label = link_label(
                        link.form,
                        &link.target,
                        link.title.as_deref(),
                        link.continues,
                    );
                    self.push(depth, label, &link.span);
                    self.located_inlines(depth + 1, &link.children);
                }
                located::Inline::Strong(strong) => {
                    let label = format!("Strong{}", continues_label(strong.continues));
                    self.push(depth, label, &strong.span);
                    self.located_inlines(depth + 1, &strong.children);
                }
                located::Inline::Text(text) => {
                    self.push(depth, format!("Text {:?}", text.value), &text.span);
                }
            }
        }
    }

    /// The rows of the readers' `mapping`, each with its span, and a pair
    /// whose key is written more than once with how many more times.
    fn located_mapping(&mut self, depth: usize, mapping: &located::Mapping) {
        self.push(depth, "Mapping".to_owned(), &mapping.span);
        for (key, pair) in &mapping.pairs {
            let merged = !pair.repeats.is_empty();
            let label = if merged {
                format!("Pair key={key} repeats={}", pair.repeats.len())
            } else {
                format!("Pair key={key}")
            };
            self.rows.push(Row {
                depth: depth + 1,
                label,
                merged,
                span: Some(pair.span.clone()),
                spread: false,
            });
            self.located_value(depth + 2, &pair.value);
        }
    }

    fn located_markdown_block<E>(&mut self, depth: usize, block: &located::MarkdownBlock<E>)
    where
        E: Extension,
    {
        self.push(depth, "MarkdownBlock".to_owned(), &block.span);
        for section in &block.sections {
            self.located_section(depth + 1, section);
        }
    }

    fn located_section<E>(&mut self, depth: usize, section: &located::Section<E>)
    where
        E: Extension,
    {
        self.push(depth, "Section".to_owned(), &section.span);
        if let Some(heading) = &section.heading {
            self.push(depth + 1, "Heading".to_owned(), &heading.span);
            self.located_inlines(depth + 2, &heading.children);
        }
        for block in &section.blocks {
            self.located_block(depth + 1, block);
        }
        for subsection in &section.subsections {
            self.located_section(depth + 1, subsection);
        }
    }

    fn located_value(&mut self, depth: usize, value: &located::Value) {
        match value {
            located::Value::Mapping(mapping) => self.located_mapping(depth, mapping),
            located::Value::Scalar(scalar) => {
                let label = value_label(&scalar.value.clone().into_model());
                self.push(depth, label, &scalar.span);
            }
            located::Value::Sequence(sequence) => {
                self.push(depth, "Sequence".to_owned(), &sequence.span);
                for item in &sequence.items {
                    self.located_value(depth + 1, item);
                }
            }
        }
    }

    /// The rows of the model's `mapping`, which covers nothing.
    fn mapping(&mut self, depth: usize, mapping: &Mapping) {
        self.push_unlocated(depth, "Mapping".to_owned());
        for (key, value) in mapping {
            self.push_unlocated(depth + 1, format!("Pair key={key}"));
            self.value(depth + 2, value);
        }
    }

    fn markdown_block<E>(&mut self, depth: usize, block: &MarkdownBlock<E>)
    where
        E: Extension,
    {
        self.push_unlocated(depth, "MarkdownBlock".to_owned());
        for section in &block.sections {
            self.section(depth + 1, section);
        }
    }

    fn push(&mut self, depth: usize, label: String, span: &Range<usize>) {
        self.rows.push(Row {
            depth,
            label,
            merged: false,
            span: Some(span.clone()),
            spread: false,
        });
    }

    fn push_unlocated(&mut self, depth: usize, label: String) {
        self.rows.push(Row {
            depth,
            label,
            merged: false,
            span: None,
            spread: false,
        });
    }

    fn raw_document<E>(&mut self, document: &RawDocument<E>)
    where
        E: Extension,
    {
        match document {
            RawDocument::Invalid(invalid) => {
                self.push_unlocated(0, "InvalidDocument".to_owned());
                let label = format!(
                    "InvalidFrontmatter format={:?} error={:?}",
                    invalid.frontmatter.format, invalid.frontmatter.error
                );
                self.push_unlocated(1, label);
                self.markdown_block(1, &invalid.body);
            }
            RawDocument::Valid(valid) => self.document(valid),
        }
    }

    fn section<E>(&mut self, depth: usize, section: &Section<E>)
    where
        E: Extension,
    {
        self.push_unlocated(depth, "Section".to_owned());
        if let Some(heading) = &section.heading {
            self.push_unlocated(depth + 1, "Heading".to_owned());
            self.inlines(depth + 2, &heading.children);
        }
        for block in &section.blocks {
            self.block(depth + 1, block);
        }
        for subsection in &section.subsections {
            self.section(depth + 1, subsection);
        }
    }

    fn value(&mut self, depth: usize, value: &Value) {
        if let Value::Mapping(mapping) = value {
            self.mapping(depth, mapping);
            return;
        }
        self.push_unlocated(depth, value_label(value));
        if let Value::Sequence(items) = value {
            for item in items {
                self.value(depth + 1, item);
            }
        }
    }
}

/// Parses `source` and checks the model's tree against `expected`, an
/// outline, and that every span of the located tree nests.
fn check(source: &str, expected: &str) {
    assert_nested(source, &located_rows(source));
    let mut rows = Rows {
        rows: Vec::new(),
        source,
    };
    rows.raw_document(&parse(source).document);
    let actual = outline(&rows.rows);
    assert!(
        actual == expected,
        "the tree differs.\n--- expected\n{expected}\n--- actual\n{actual}"
    );
}

/// The frontmatter of `source`, one of which no entry reads.
fn broken(source: &str) -> located::BrokenFrontmatter {
    parse_located::<Markdown>(source, &ParseOptions::default())
        .frontmatter
        .unwrap()
        .unwrap_err()
}

/// Parses `source` into its located tree and checks, node by node, the
/// source text each span covers.
fn check_spans(source: &str, expected: &[(&str, &str)]) {
    let found = located_rows(source);
    let actual: Vec<(&str, &str)> = found
        .iter()
        .map(|row| (row.label.as_str(), &source[row.span.clone().unwrap()]))
        .collect();
    assert_eq!(actual, expected);
}

/// Runs `test` on a thread with a 2 MB stack, the stack a test thread gets.
pub fn on_two_megabytes<F>(test: F)
where
    F: FnOnce() + Send + 'static,
{
    thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

/// One line per node, indented two spaces under its parent.
fn outline(rows: &[Row]) -> String {
    let mut text = String::new();
    for row in rows {
        text.push_str(&"  ".repeat(row.depth));
        text.push_str(&row.label);
        text.push('\n');
    }
    text
}

/// The rows of the located tree `source` parses into, each with its span.
fn located_rows(source: &str) -> Vec<Row> {
    let document = parse_located::<Markdown>(source, &ParseOptions::default());
    assert_eq!(document.span, 0..source.len());
    let mut rows = Rows {
        rows: Vec::new(),
        source,
    };
    rows.located_document(&document);
    rows.rows
}

/// Every span lies inside its parent's span and after its previous sibling.
/// Under a pair whose key is written more than once, the values come from
/// several places, so each only lies inside the frontmatter. The cells of a
/// row written over several lines interleave, so each only lies inside its
/// row. Every node of a copy an alias made spans the alias.
fn assert_nested(source: &str, rows: &[Row]) {
    let mut open: Vec<Range<usize>> = Vec::new();
    let mut merged: Option<(usize, Range<usize>)> = None;
    for row in rows {
        let span = row.span.as_ref().unwrap();
        let label = &row.label;
        assert!(
            span.start <= span.end
                && source.is_char_boundary(span.start)
                && source.is_char_boundary(span.end),
            "{label} has a bad span"
        );
        if let Some((depth, frontmatter)) = &merged {
            if row.depth > *depth {
                assert!(
                    frontmatter.start <= span.start && span.end <= frontmatter.end,
                    "{label} is outside the frontmatter"
                );
                continue;
            }
            merged = None;
        }
        open.truncate(row.depth + 1);
        if let Some(parent) = row.depth.checked_sub(1).and_then(|depth| open.get(depth)) {
            assert!(
                parent.start <= span.start && span.end <= parent.end,
                "{label} is outside its parent"
            );
        }
        if let Some(sibling) = open.get(row.depth) {
            assert!(
                row.spread || sibling.end <= span.start || sibling == span,
                "{label} overlaps the node before it"
            );
        }
        open.truncate(row.depth);
        open.push(span.clone());
        if row.merged {
            merged = open
                .get(1)
                .map(|frontmatter| (row.depth, frontmatter.clone()));
        }
    }
}

/// The frontmatter `source` opens with as its reader reads it, with spans.
fn located_frontmatter(source: &str) -> located::Mapping {
    read_again(source, &located_frontmatter_node(source))
}

/// The located frontmatter `source` opens with, one that reads.
fn located_frontmatter_node(source: &str) -> located::Frontmatter {
    parse_located::<Markdown>(source, &ParseOptions::default())
        .frontmatter
        .unwrap()
        .unwrap()
}

/// The frontmatter of `source` read again by its reader, which keeps the
/// span of each value and every place each key is written.
fn read_again(source: &str, frontmatter: &located::Frontmatter) -> located::Mapping {
    let content = line_at(source, frontmatter.span.start).next..frontmatter.span.end - 3;
    let read = match frontmatter.format {
        FrontmatterFormat::Toml => super::toml::read(source, content),
        FrontmatterFormat::Yaml => super::yaml::read(source, content),
    };
    read.unwrap().0
}

/// The document `source` parses into, whose frontmatter, if it has one,
/// reads.
fn valid(source: &str) -> Document {
    let read = match parse(source).document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => Some(valid),
    };
    read.unwrap()
}

/// The body of `document` when it is valid.
const fn valid_body<E>(document: &RawDocument<E>) -> Option<&MarkdownBlock<E>>
where
    E: Extension,
{
    match document {
        RawDocument::Invalid(_) => None,
        RawDocument::Valid(valid) => Some(&valid.body),
    }
}

/// `CodeBlock`, its language, its args, its value, and ` claimed` when an
/// extension claimed it.
fn code_block_label(language: Option<&str>, args: &Args, value: &str, claimed: bool) -> String {
    let named = language.map_or_else(String::new, |written| format!(" language={written:?}"));
    let mark = if claimed { " claimed" } else { "" };
    format!("CodeBlock{named}{} value={value:?}{mark}", args_label(args))
}

/// ` args=Text "..."`, or ` args=Toml {...}` with the pairs as the writer
/// writes them, else nothing when the args are an empty mapping.
fn args_label(args: &Args) -> String {
    match args {
        Args::Text(text) => format!(" args=Text {text:?}"),
        Args::Toml(mapping) if mapping.is_empty() => String::new(),
        Args::Toml(_) => {
            let block = CodeBlock::<Markdown> {
                args: args.clone(),
                kind: Kind(None),
                language: None,
                value: String::new(),
            };
            let info = Writer::new().info_string(&block);
            format!(" args=Toml {{{}}}", info.trim_start())
        }
    }
}

/// `List`, its first number when it is ordered, and ` loose` when it is.
fn list_label(start: Option<u64>, loose: bool) -> String {
    let numbered = start.map_or_else(String::new, |first| format!(" start={first}"));
    let mark = if loose { " loose" } else { "" };
    format!("List{numbered}{mark}")
}

/// `Table`, its syntax and the alignment of each of its columns.
fn table_label(format: TableFormat, alignments: &[Alignment]) -> String {
    let names: Vec<String> = alignments
        .iter()
        .map(|alignment| format!("{alignment:?}"))
        .collect();
    format!("Table format={format:?} alignments={}", names.join(","))
}

fn image_label(
    form: LinkForm,
    target: &str,
    title: Option<&str>,
    alt: &str,
    continues: Continues,
) -> String {
    let shown = if alt.is_empty() {
        String::new()
    } else {
        format!(" alt={alt:?}")
    };
    format!(
        "Image form={form:?} target={target:?}{}{shown}{}",
        title_label(title),
        continues_label(continues)
    )
}

fn link_label(form: LinkForm, target: &str, title: Option<&str>, continues: Continues) -> String {
    format!(
        "Link form={form:?} target={target:?}{}{}",
        title_label(title),
        continues_label(continues)
    )
}

/// ` continues=...` for a piece of a node cut at a line end, else nothing.
const fn continues_label(continues: Continues) -> &'static str {
    match continues {
        Continues::Both => " continues=both",
        Continues::Neither => "",
        Continues::Next => " continues=next",
        Continues::Previous => " continues=previous",
    }
}

/// ` title="..."` for a link or an image with a title, else nothing.
fn title_label(title: Option<&str>) -> String {
    title.map_or_else(String::new, |written| format!(" title={written:?}"))
}

/// `Mapping`, `Sequence`, or `Scalar` and the variant and value of any other
/// value.
fn value_label(value: &Value) -> String {
    match value {
        Value::Boolean(boolean) => format!("Scalar Boolean {boolean}"),
        Value::Date(date) => format!("Scalar Date {}", date_label(*date)),
        Value::DateTime(date_time) => format!("Scalar DateTime {}", date_time_label(*date_time)),
        Value::Float(float) => format!("Scalar Float {float}"),
        Value::Integer(integer) => format!("Scalar Integer {integer}"),
        Value::Mapping(_) => "Mapping".to_owned(),
        Value::Null(()) => "Scalar Null".to_owned(),
        Value::OffsetDateTime(date_time) => {
            format!(
                "Scalar OffsetDateTime {}",
                offset_date_time_label(date_time)
            )
        }
        Value::Sequence(_) => "Sequence".to_owned(),
        Value::String(string) => format!("Scalar String {string:?}"),
        Value::Time(time) => format!("Scalar Time {}", time_label(*time)),
    }
}

fn date_label(date: NaiveDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// chrono holds a leap second as second 59 with a second or more of
/// nanoseconds; it prints as second 60.
fn time_label(time: NaiveTime) -> String {
    let leap = time.nanosecond() / 1_000_000_000;
    let nanosecond = time.nanosecond() % 1_000_000_000;
    let clock = format!(
        "{:02}:{:02}:{:02}",
        time.hour(),
        time.minute(),
        time.second() + leap
    );
    if nanosecond == 0 {
        clock
    } else {
        format!("{clock}.{nanosecond:09}")
    }
}

fn date_time_label(date_time: NaiveDateTime) -> String {
    format!(
        "{}T{}",
        date_label(date_time.date()),
        time_label(date_time.time())
    )
}

fn offset_date_time_label(date_time: &DateTime<FixedOffset>) -> String {
    let seconds = date_time.offset().local_minus_utc();
    let offset = if seconds == 0_i32 {
        "Z".to_owned()
    } else {
        let sign = if seconds < 0_i32 { '-' } else { '+' };
        let minutes = seconds.unsigned_abs() / 60;
        format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    };
    format!("{}{offset}", date_time_label(date_time.naive_local()))
}
