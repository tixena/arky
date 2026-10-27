//! The tree written from its archived form, read in place: the writer's node walk over
//! rkyv's archived types, through the same text and leaf helpers. An archived tree holds no
//! extension node, so each node is written in its plain form.

use core::mem;

use rkyv::collections::swiss_table::ArchivedIndexMap;
use rkyv::rancor::{Error, Strategy};
use rkyv::string::ArchivedString;
use rkyv::with::DeserializeWith;

use crate::parser::archive::{AsDays, AsDaysAndSeconds, AsLocalAndOffset, AsSecondsOfDay};
use crate::parser::ast::{
    Alignment, ArchivedAlignment, ArchivedArgs, ArchivedBlock, ArchivedCode, ArchivedCodeBlock,
    ArchivedContinues, ArchivedFrontmatterFormat, ArchivedHeading, ArchivedHtml, ArchivedImage,
    ArchivedInline, ArchivedLink, ArchivedLinkForm, ArchivedLinkInline, ArchivedList,
    ArchivedMarkdownBlock, ArchivedParagraph, ArchivedRawDocument, ArchivedSection, ArchivedTable,
    ArchivedTableCell, ArchivedTableRow, ArchivedText, ArchivedValue, Continues, FrontmatterFormat,
    LinkForm,
};
use crate::writer::inline::{EMPHASIS, Pieces, STRONG, Segments};
use crate::writer::table::{Row, lay_out};
use crate::writer::{Scalar, Shape, WriteError, Writer, as_read, info, toml, yaml};

/// A frontmatter mapping as rkyv archives it.
type ArchivedMapping = ArchivedIndexMap<ArchivedString, ArchivedValue>;

/// An archived node of a line, or of a link's text, as the writer reads it.
trait ArchivedNode: Sized {
    fn view(&self) -> ArchivedView<'_, Self>;
}

/// An archived inline node, its children typed as the node that holds them.
enum ArchivedView<'node, N> {
    Code(&'node ArchivedCode),
    Emphasis {
        children: &'node [N],
        continues: Continues,
    },
    Html(&'node ArchivedHtml),
    Image(&'node ArchivedImage),
    LineBreak,
    Link(&'node ArchivedLink),
    Strong {
        children: &'node [N],
        continues: Continues,
    },
    Text(&'node ArchivedText),
}

impl ArchivedNode for ArchivedInline {
    fn view(&self) -> ArchivedView<'_, Self> {
        match self {
            Self::Code(code) => ArchivedView::Code(code),
            Self::Emphasis(emphasis) => ArchivedView::Emphasis {
                children: &emphasis.children,
                continues: continues(&emphasis.continues),
            },
            Self::Html(html) => ArchivedView::Html(html),
            Self::Image(image) => ArchivedView::Image(image),
            Self::LineBreak(_) => ArchivedView::LineBreak,
            Self::Link(link) => ArchivedView::Link(link),
            Self::Strong(strong) => ArchivedView::Strong {
                children: &strong.children,
                continues: continues(&strong.continues),
            },
            Self::Text(text) => ArchivedView::Text(text),
        }
    }
}

impl ArchivedNode for ArchivedLinkInline {
    fn view(&self) -> ArchivedView<'_, Self> {
        match self {
            Self::Code(code) => ArchivedView::Code(code),
            Self::Emphasis(emphasis) => ArchivedView::Emphasis {
                children: &emphasis.children,
                continues: continues(&emphasis.continues),
            },
            Self::Html(html) => ArchivedView::Html(html),
            Self::Image(image) => ArchivedView::Image(image),
            Self::LineBreak(_) => ArchivedView::LineBreak,
            Self::Strong(strong) => ArchivedView::Strong {
                children: &strong.children,
                continues: continues(&strong.continues),
            },
            Self::Text(text) => ArchivedView::Text(text),
        }
    }
}

/// Writes `document` as a file holds it, as `Writer::raw_document` does.
pub fn raw_document(writer: &mut Writer, document: &ArchivedRawDocument) {
    let (frontmatter, file_start) = match document {
        ArchivedRawDocument::Invalid(invalid) => (
            as_read(
                &invalid.frontmatter.text,
                frontmatter_format(&invalid.frontmatter.format),
            ),
            false,
        ),
        ArchivedRawDocument::Valid(valid) => {
            let written = match (valid.frontmatter.as_ref(), writer.frontmatter) {
                (None, _) => Ok(String::new()),
                (Some(mapping), FrontmatterFormat::Toml) => toml_frontmatter(mapping),
                (Some(mapping), FrontmatterFormat::Yaml) => yaml_frontmatter(mapping),
            };
            (written, valid.frontmatter.is_none())
        }
    };
    writer.file(frontmatter, file_start, |body| match document {
        ArchivedRawDocument::Invalid(invalid) => markdown_node(body, &invalid.body),
        ArchivedRawDocument::Valid(valid) => {
            body.standalone(|inner| markdown_node(inner, &valid.body));
        }
    });
}

fn block(writer: &mut Writer, block: &ArchivedBlock) {
    writer.standalone(|inner| match block {
        ArchivedBlock::BlockQuote(quote) => inner.block_quote(&quote.value),
        ArchivedBlock::CodeBlock(code) => {
            let info = info_string(inner, code);
            inner.code_block(&info, &code.value);
        }
        ArchivedBlock::Html(html) => inner.put(&html.value),
        ArchivedBlock::List(node) => list(inner, node),
        ArchivedBlock::Paragraph(node) => paragraph(inner, node),
        ArchivedBlock::Table(node) => table(inner, node),
        ArchivedBlock::ThematicBreak(_) => inner.thematic_break(),
    });
}

fn heading(writer: &mut Writer, node: &ArchivedHeading) {
    writer.heading_line(
        &pieces(&node.children),
        holds_line_break(&node.children),
        node.children.is_empty(),
        |segments, _| nodes(segments, &node.children, false),
    );
}

/// The info string of `code`, keeping the error its `Toml` args meet.
fn info_string(writer: &mut Writer, code: &ArchivedCodeBlock) -> String {
    let language = code.language.as_deref();
    match &code.args {
        ArchivedArgs::Text(text) => info(language, text),
        ArchivedArgs::Toml(mapping) => {
            let (text, error) = toml_args(mapping);
            writer.record(error);
            info(language, &text)
        }
    }
}

fn list(writer: &mut Writer, node: &ArchivedList) {
    let start = node.start.as_ref().map(|first| first.to_native());
    writer.items(node.items.iter(), node.loose, start, |inner, item| {
        inner.standalone(|body| markdown_node(body, &item.body));
    });
}

/// Writes `node` on lines of its own, its levels counted from one.
fn markdown_node(writer: &mut Writer, node: &ArchivedMarkdownBlock) {
    writer.standalone(|inner| inner.sections(node.sections.iter(), section));
}

fn paragraph(writer: &mut Writer, node: &ArchivedParagraph) {
    let mut found = Pieces::default();
    for line in node.lines.iter() {
        add_pieces(&mut found, &line.children);
    }
    writer.put_lines(|_, pipes| {
        let mut segments = Segments::new(&found);
        if pipes {
            segments.escape_pipes();
        }
        let mut lines = Vec::with_capacity(node.lines.len());
        let mut text_led = false;
        for line in node.lines.iter() {
            let first = lines.is_empty();
            nodes(&mut segments, &line.children, false);
            let (text, led) = segments.line(first);
            if text.is_empty() {
                continue;
            }
            if first {
                text_led = led;
            }
            lines.push(text);
        }
        (lines, text_led)
    });
}

/// The heading, or the empty heading line a section with none takes, then
/// the blocks, then the subsections one level deeper.
fn plain_section(writer: &mut Writer, node: &ArchivedSection) {
    let place = mem::take(&mut writer.place);
    match node.heading.as_ref() {
        Some(line) => writer.standalone(|inner| heading(inner, line)),
        None => writer.no_heading(place, !node.blocks.is_empty(), !node.subsections.is_empty()),
    }
    let mut previous: Option<&ArchivedList> = None;
    for each in node.blocks.iter() {
        let current = as_list(each);
        writer.pick_marker(
            previous
                .zip(current)
                .is_some_and(|(before, after)| before.start.is_some() == after.start.is_some()),
        );
        block(writer, each);
        previous = current;
    }
    for (index, subsection) in node.subsections.iter().enumerate() {
        writer.subsection(index, |inner| section(inner, subsection));
    }
}

fn section(writer: &mut Writer, node: &ArchivedSection) {
    writer.standalone(|inner| plain_section(inner, node));
}

/// `block` when it is a list.
const fn as_list(block: &ArchivedBlock) -> Option<&ArchivedList> {
    match block {
        ArchivedBlock::List(node) => Some(node),
        ArchivedBlock::BlockQuote(_)
        | ArchivedBlock::CodeBlock(_)
        | ArchivedBlock::Html(_)
        | ArchivedBlock::Paragraph(_)
        | ArchivedBlock::Table(_)
        | ArchivedBlock::ThematicBreak(_) => None,
    }
}

/// Adds the code span pieces among `children` and their children.
fn add_pieces<N>(found: &mut Pieces, children: &[N])
where
    N: ArchivedNode,
{
    for node in children {
        match node.view() {
            ArchivedView::Code(code) => found.code(&code.value, continues(&code.continues)),
            ArchivedView::Emphasis {
                children: inner,
                continues: _,
            }
            | ArchivedView::Strong {
                children: inner,
                continues: _,
            } => add_pieces(found, inner),
            ArchivedView::Link(anchor) if matches!(anchor.form, ArchivedLinkForm::Markdown) => {
                add_pieces(found, &anchor.children);
            }
            ArchivedView::Html(_)
            | ArchivedView::Image(_)
            | ArchivedView::LineBreak
            | ArchivedView::Link(_)
            | ArchivedView::Text(_) => {}
        }
    }
}

/// Whether `children` hold a line break, among them or among the children
/// of an emphasis, strong or markdown link they hold.
fn holds_line_break<N>(children: &[N]) -> bool
where
    N: ArchivedNode,
{
    children.iter().any(|node| match node.view() {
        ArchivedView::Emphasis {
            children: inner,
            continues: _,
        }
        | ArchivedView::Strong {
            children: inner,
            continues: _,
        } => holds_line_break(inner),
        ArchivedView::LineBreak => true,
        ArchivedView::Link(anchor) => {
            matches!(anchor.form, ArchivedLinkForm::Markdown) && holds_line_break(&anchor.children)
        }
        ArchivedView::Code(_)
        | ArchivedView::Html(_)
        | ArchivedView::Image(_)
        | ArchivedView::Text(_) => false,
    })
}

/// `[text](target "title")`, or `[[target|text]]` for a wiki link.
fn link(segments: &mut Segments, node: &ArchivedLink) {
    match link_form(&node.form) {
        LinkForm::Markdown => segments.markdown_link(
            continues(&node.continues),
            &node.target,
            node.title.as_deref(),
            |inner| nodes(inner, &node.children, true),
        ),
        LinkForm::Wiki => {
            let alias: String = node.children.iter().filter_map(text_of).collect();
            segments.wiki_link(&node.target, &alias);
        }
    }
}

/// Adds `node`. `in_link` is set between a link's brackets.
fn node<N>(segments: &mut Segments, node: &N, in_link: bool)
where
    N: ArchivedNode,
{
    match node.view() {
        ArchivedView::Code(code) => segments.code(&code.value, continues(&code.continues)),
        ArchivedView::Emphasis {
            children,
            continues,
        } => {
            segments.wrapped(continues, EMPHASIS, |inner| {
                nodes(inner, children, in_link);
            });
        }
        ArchivedView::Html(html) => segments.html(&html.value),
        ArchivedView::Image(image) => segments.image(
            link_form(&image.form),
            continues(&image.continues),
            &image.alt,
            &image.target,
            image.title.as_deref(),
        ),
        ArchivedView::LineBreak => segments.line_break(),
        ArchivedView::Link(anchor) => link(segments, anchor),
        ArchivedView::Strong {
            children,
            continues,
        } => {
            segments.wrapped(continues, STRONG, |inner| {
                nodes(inner, children, in_link);
            });
        }
        ArchivedView::Text(text) => segments.text(&text.value, in_link),
    }
}

/// Adds `children`, each run of emphases or of strongs next to each other
/// and whole on their line inside one pair of marks.
fn nodes<N>(segments: &mut Segments, children: &[N], in_link: bool)
where
    N: ArchivedNode,
{
    let touching_spans = |left: &N, right: &N| {
        whole_span(left)
            .zip(whole_span(right))
            .is_some_and(|((left_mark, _), (right_mark, _))| left_mark == right_mark)
    };
    for run in children.chunk_by(touching_spans) {
        match run {
            [single] => node(segments, single, in_link),
            _ => touching(segments, run, in_link),
        }
    }
}

/// The code span pieces among `children`.
fn pieces<N>(children: &[N]) -> Pieces
where
    N: ArchivedNode,
{
    let mut found = Pieces::default();
    add_pieces(&mut found, children);
    found
}

/// The text of `node`, when it is text.
fn text_of<N>(node: &N) -> Option<&str>
where
    N: ArchivedNode,
{
    match node.view() {
        ArchivedView::Text(text) => Some(&text.value),
        ArchivedView::Code(_)
        | ArchivedView::Emphasis {
            children: _,
            continues: _,
        }
        | ArchivedView::Html(_)
        | ArchivedView::Image(_)
        | ArchivedView::LineBreak
        | ArchivedView::Link(_)
        | ArchivedView::Strong {
            children: _,
            continues: _,
        } => None,
    }
}

/// `run`, emphases or strongs next to each other, inside one pair of their
/// mark, the children of each one after the other.
fn touching<N>(segments: &mut Segments, run: &[N], in_link: bool)
where
    N: ArchivedNode,
{
    let spans = run.iter().filter_map(whole_span);
    let mark = spans.clone().next().map_or("", |(mark, _)| mark);
    segments.wrapped(Continues::Neither, mark, |inner| {
        for (_, children) in spans {
            nodes(inner, children, in_link);
        }
    });
}

/// The mark and the children of `node`, when it is an emphasis or a strong
/// that is whole on its line.
fn whole_span<N>(node: &N) -> Option<(&'static str, &[N])>
where
    N: ArchivedNode,
{
    let (mark, children, continues) = match node.view() {
        ArchivedView::Emphasis {
            children,
            continues,
        } => (EMPHASIS, children, continues),
        ArchivedView::Strong {
            children,
            continues,
        } => (STRONG, children, continues),
        ArchivedView::Code(_)
        | ArchivedView::Html(_)
        | ArchivedView::Image(_)
        | ArchivedView::LineBreak
        | ArchivedView::Link(_)
        | ArchivedView::Text(_) => return None,
    };
    (continues == Continues::Neither).then_some((mark, children))
}

/// The text of `node`'s content, with its levels counted from one. `writer`
/// keeps the error met while writing it.
fn cell(writer: &mut Writer, node: &ArchivedTableCell) -> String {
    let mut inner = writer.inner();
    inner.standalone(|body| markdown_node(body, &node.body));
    writer.keep_error(&mut inner);
    inner.finish().trim_end_matches('\n').to_owned()
}

/// The cells of `node`, `width` of them, empty ones added at the end.
fn row(writer: &mut Writer, node: &ArchivedTableRow, width: usize) -> Row {
    let mut cells: Vec<String> = node.cells.iter().map(|each| cell(writer, each)).collect();
    cells.resize(width.max(cells.len()), String::new());
    Row::Cells(cells)
}

fn table(writer: &mut Writer, node: &ArchivedTable) {
    let width = node
        .head
        .iter()
        .chain(node.rows.iter())
        .map(|each| each.cells.len())
        .chain([node.alignments.len()])
        .max()
        .unwrap_or_default();
    let head: Vec<Row> = node
        .head
        .iter()
        .map(|each| row(writer, each, width))
        .collect();
    let rows: Vec<Row> = node
        .rows
        .iter()
        .map(|each| row(writer, each, width))
        .collect();
    let alignments: Vec<Alignment> = node.alignments.iter().map(alignment).collect();
    lay_out(writer, &alignments, width, &head, &rows);
}

/// `mapping` between `---` lines, or an error naming the key of the first
/// date or time out of chrono's range.
fn yaml_frontmatter(mapping: &ArchivedMapping) -> Result<String, WriteError> {
    let mut content = String::new();
    yaml_block_mapping(&mut content, mapping, 0, false, &[])?;
    Ok(yaml::delimited(&content))
}

/// `value` after a key's `:` or, `on_entry`, after a list entry's `-`.
fn yaml_after_indicator(
    out: &mut String,
    value: &ArchivedValue,
    indent: usize,
    on_entry: bool,
    path: &[&str],
) -> Result<(), WriteError> {
    let opening = if on_entry { ' ' } else { '\n' };
    match value {
        ArchivedValue::Mapping(mapping) if !mapping.is_empty() => {
            out.push(opening);
            yaml_block_mapping(out, mapping, indent, on_entry, path)
        }
        ArchivedValue::Sequence(sequence) if yaml_holds_mapping(sequence) => {
            out.push(opening);
            yaml_block_sequence(out, sequence, indent, on_entry, path)
        }
        ArchivedValue::Boolean(_)
        | ArchivedValue::Date(_)
        | ArchivedValue::DateTime(_)
        | ArchivedValue::Float(_)
        | ArchivedValue::Integer(_)
        | ArchivedValue::Mapping(_)
        | ArchivedValue::Null(())
        | ArchivedValue::OffsetDateTime(_)
        | ArchivedValue::Sequence(_)
        | ArchivedValue::String(_)
        | ArchivedValue::Time(_) => {
            out.push(' ');
            yaml_flow(out, value, false, path)?;
            out.push('\n');
            Ok(())
        }
    }
}

/// The pairs of `mapping`, each key at column `indent`.
fn yaml_block_mapping(
    out: &mut String,
    mapping: &ArchivedMapping,
    indent: usize,
    on_entry: bool,
    path: &[&str],
) -> Result<(), WriteError> {
    for (index, (key, value)) in mapping.iter().enumerate() {
        if index > 0 || !on_entry {
            yaml::push_indent(out, indent);
        }
        yaml::push_text(out, key, false);
        out.push(':');
        yaml_after_indicator(out, value, indent + 2, false, &toml::joined(path, key))?;
    }
    Ok(())
}

/// The items of `sequence`, each `-` at column `indent`.
fn yaml_block_sequence(
    out: &mut String,
    sequence: &[ArchivedValue],
    indent: usize,
    on_entry: bool,
    path: &[&str],
) -> Result<(), WriteError> {
    for (index, item) in sequence.iter().enumerate() {
        if index > 0 || !on_entry {
            yaml::push_indent(out, indent);
        }
        out.push('-');
        yaml_after_indicator(out, item, indent + 2, true, path)?;
    }
    Ok(())
}

/// `value` on one line, in flow form, inside a flow collection when
/// `in_flow`.
fn yaml_flow(
    out: &mut String,
    value: &ArchivedValue,
    in_flow: bool,
    path: &[&str],
) -> Result<(), WriteError> {
    match shape(value, path)? {
        Shape::Mapping(mapping) => {
            out.push('{');
            for (index, (key, item)) in mapping.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                yaml::push_text(out, key, true);
                out.push_str(": ");
                yaml_flow(out, item, true, &toml::joined(path, key))?;
            }
            out.push('}');
        }
        Shape::Scalar(scalar) => yaml::push_scalar(out, scalar, in_flow),
        Shape::Sequence(sequence) => {
            out.push('[');
            for (index, item) in sequence.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                yaml_flow(out, item, true, path)?;
            }
            out.push(']');
        }
    }
    Ok(())
}

/// Whether `sequence` holds a mapping, in itself or in a list inside it.
fn yaml_holds_mapping(sequence: &[ArchivedValue]) -> bool {
    sequence.iter().any(|item| {
        matches!(item, ArchivedValue::Mapping(_))
            || matches!(item, ArchivedValue::Sequence(inner) if yaml_holds_mapping(inner))
    })
}

/// The pairs of `mapping` on one line: a code block's args. A pair that
/// fails is left out, and the error of the first one is given.
fn toml_args(mapping: &ArchivedMapping) -> (String, Option<WriteError>) {
    let mut out = String::new();
    let mut error = None;
    for (key, written) in mapping.iter() {
        let mut value = String::new();
        match toml_inline(&mut value, written, &[key]) {
            Ok(()) => {
                if !out.is_empty() {
                    out.push_str(", ");
                }
                toml::push_key(&mut out, key);
                out.push_str(" = ");
                out.push_str(&value);
            }
            Err(found) => {
                error = error.or(Some(found));
            }
        }
    }
    (out, error)
}

/// `mapping` between `+++` lines, or an error naming the key of the first
/// null, or of the first date or time out of chrono's range.
fn toml_frontmatter(mapping: &ArchivedMapping) -> Result<String, WriteError> {
    let mut content = String::new();
    toml_table(&mut content, mapping, &[])?;
    Ok(toml::delimited(&content))
}

/// `value` on one line: an inline table, an array, or a scalar.
fn toml_inline(out: &mut String, value: &ArchivedValue, path: &[&str]) -> Result<(), WriteError> {
    match shape(value, path)? {
        Shape::Mapping(mapping) => {
            out.push('{');
            for (index, (key, item)) in mapping.iter().enumerate() {
                out.push_str(if index > 0 { ", " } else { " " });
                toml::push_key(out, key);
                out.push_str(" = ");
                toml_inline(out, item, &toml::joined(path, key))?;
            }
            out.push_str(if mapping.is_empty() { "}" } else { " }" });
        }
        Shape::Scalar(scalar) => toml::push_scalar(out, scalar, path)?,
        Shape::Sequence(sequence) => {
            out.push('[');
            for (index, item) in sequence.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                toml_inline(out, item, path)?;
            }
            out.push(']');
        }
    }
    Ok(())
}

/// Whether `value` is written under a header: a mapping, or a list of
/// mappings.
fn toml_is_header(value: &ArchivedValue) -> bool {
    matches!(value, ArchivedValue::Mapping(_)) || toml_tables(value).is_some()
}

/// The plain keys of `mapping`, then a header for each mapping it holds and
/// for each mapping of a list of mappings. `path` holds the keys of the
/// table.
fn toml_table(
    out: &mut String,
    mapping: &ArchivedMapping,
    path: &[&str],
) -> Result<(), WriteError> {
    for (key, value) in mapping.iter() {
        if !toml_is_header(value) {
            toml::push_key(out, key);
            out.push_str(" = ");
            toml_inline(out, value, &toml::joined(path, key))?;
            out.push('\n');
        }
    }
    for (key, value) in mapping.iter() {
        let inner = toml::joined(path, key);
        if let ArchivedValue::Mapping(nested) = value {
            toml::header(out, &inner, false);
            toml_table(out, nested, &inner)?;
        }
        for item in toml_tables(value).unwrap_or_default() {
            toml::header(out, &inner, true);
            toml_table(out, item, &inner)?;
        }
    }
    Ok(())
}

/// The mappings of a list of one mapping or more that holds nothing else.
fn toml_tables(value: &ArchivedValue) -> Option<Vec<&ArchivedMapping>> {
    let ArchivedValue::Sequence(sequence) = value else {
        return None;
    };
    if sequence.is_empty() {
        return None;
    }
    sequence
        .iter()
        .map(|item| match item {
            ArchivedValue::Mapping(mapping) => Some(mapping),
            ArchivedValue::Boolean(_)
            | ArchivedValue::Date(_)
            | ArchivedValue::DateTime(_)
            | ArchivedValue::Float(_)
            | ArchivedValue::Integer(_)
            | ArchivedValue::Null(())
            | ArchivedValue::OffsetDateTime(_)
            | ArchivedValue::Sequence(_)
            | ArchivedValue::String(_)
            | ArchivedValue::Time(_) => None,
        })
        .collect()
}

/// The shape of `value`, its dates and times read through their adapters,
/// or an error naming `path`, its key, when one is out of chrono's range.
fn shape<'value>(
    value: &'value ArchivedValue,
    path: &[&str],
) -> Result<Shape<'value, ArchivedMapping, ArchivedValue>, WriteError> {
    let scalar = match value {
        ArchivedValue::Boolean(boolean) => Ok(Scalar::Boolean(*boolean)),
        ArchivedValue::Date(days) => read::<AsDays, _, _>(days).map(Scalar::Date),
        ArchivedValue::DateTime(date_time) => {
            read::<AsDaysAndSeconds, _, _>(date_time).map(Scalar::DateTime)
        }
        ArchivedValue::Float(float) => Ok(Scalar::Float(float.to_native())),
        ArchivedValue::Integer(integer) => Ok(Scalar::Integer(integer.to_native())),
        ArchivedValue::Mapping(mapping) => return Ok(Shape::Mapping(mapping)),
        ArchivedValue::Null(()) => Ok(Scalar::Null),
        ArchivedValue::OffsetDateTime(date_time) => {
            read::<AsLocalAndOffset, _, _>(date_time).map(Scalar::OffsetDateTime)
        }
        ArchivedValue::Sequence(sequence) => return Ok(Shape::Sequence(sequence)),
        ArchivedValue::String(text) => Ok(Scalar::String(text)),
        ArchivedValue::Time(time) => read::<AsSecondsOfDay, _, _>(time).map(Scalar::Time),
    };
    scalar.map(Shape::Scalar).map_err(|error| WriteError {
        key: toml::key_of(path),
        message: error.to_string(),
    })
}

/// The chrono value `field` holds, read through the adapter `A`.
fn read<A, F, T>(field: &F) -> Result<T, Error>
where
    A: DeserializeWith<F, T, Strategy<(), Error>>,
{
    A::deserialize_with(field, Strategy::wrap(&mut ()))
}

const fn alignment(archived: &ArchivedAlignment) -> Alignment {
    match archived {
        ArchivedAlignment::Center => Alignment::Center,
        ArchivedAlignment::Default => Alignment::Default,
        ArchivedAlignment::Left => Alignment::Left,
        ArchivedAlignment::Right => Alignment::Right,
    }
}

const fn continues(archived: &ArchivedContinues) -> Continues {
    match archived {
        ArchivedContinues::Both => Continues::Both,
        ArchivedContinues::Neither => Continues::Neither,
        ArchivedContinues::Next => Continues::Next,
        ArchivedContinues::Previous => Continues::Previous,
    }
}

const fn frontmatter_format(archived: &ArchivedFrontmatterFormat) -> FrontmatterFormat {
    match archived {
        ArchivedFrontmatterFormat::Toml => FrontmatterFormat::Toml,
        ArchivedFrontmatterFormat::Yaml => FrontmatterFormat::Yaml,
    }
}

const fn link_form(archived: &ArchivedLinkForm) -> LinkForm {
    match archived {
        ArchivedLinkForm::Markdown => LinkForm::Markdown,
        ArchivedLinkForm::Wiki => LinkForm::Wiki,
    }
}
