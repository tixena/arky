//! Mirrors of the model's enums, archived through rkyv's remote derive. The
//! archived enums and resolvers rkyv generates live in this private module,
//! and `ast` names the archived enums publicly.

use alloc::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use rkyv::Place;
use rkyv::collections::swiss_table::{ArchivedIndexMap, IndexMapResolver};
use rkyv::rancor::{Fallible, Source};
use rkyv::ser::{Allocator, Writer};
use rkyv::string::ArchivedString;
use rkyv::validation::ArchiveContext;
use rkyv::with::{ArchiveWith, DeserializeWith, Map, SerializeWith, With};

use crate::bundle::BundledDocument;
use crate::parser::archive::{AsDays, AsDaysAndSeconds, AsLocalAndOffset, AsSecondsOfDay};
use crate::parser::ast;
use crate::parser::extension::{Extension, Markdown};

/// The load factor rkyv archives an `IndexMap` with.
const LOAD_FACTOR: (usize, usize) = (7, 8);

/// A `Mapping` as rkyv archives it.
type ArchivedMapping = ArchivedIndexMap<ArchivedString, ArchivedValueMirror>;

/// A `Mapping` archived as rkyv archives an `IndexMap`, each value through
/// `ValueMirror`: rkyv's `MapKV` adapts no `IndexMap`.
#[derive(Debug, Clone, Copy)]
pub struct MappingMirror;

impl ArchiveWith<ast::Mapping> for MappingMirror {
    type Archived = ArchivedMapping;
    type Resolver = IndexMapResolver;

    fn resolve_with(field: &ast::Mapping, resolver: Self::Resolver, out: Place<Self::Archived>) {
        ArchivedMapping::resolve_from_len(field.len(), LOAD_FACTOR, resolver, out);
    }
}

impl<S> SerializeWith<ast::Mapping, S> for MappingMirror
where
    S: Fallible + Allocator + Writer + ?Sized,
    S::Error: Source,
{
    fn serialize_with(
        field: &ast::Mapping,
        serializer: &mut S,
    ) -> Result<Self::Resolver, S::Error> {
        let entries = field
            .iter()
            .map(|(key, value)| (key, With::<ast::Value, ValueMirror>::cast(value)));
        ArchivedMapping::serialize_from_iter::<_, _, _, String, With<ast::Value, ValueMirror>, _>(
            entries,
            LOAD_FACTOR,
            serializer,
        )
    }
}

impl<D> DeserializeWith<ArchivedMapping, ast::Mapping, D> for MappingMirror
where
    D: Fallible + ?Sized,
    D::Error: Source,
{
    fn deserialize_with(
        field: &ArchivedMapping,
        deserializer: &mut D,
    ) -> Result<ast::Mapping, D::Error> {
        field
            .iter()
            .map(|(key, archived)| {
                let value = ValueMirror::deserialize_with(archived, deserializer)?;
                Ok((key.as_str().to_owned(), value))
            })
            .collect()
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::RawDocument<E>)]
#[rkyv(attr(non_exhaustive))]
pub enum RawDocumentMirror<E = Markdown>
where
    E: Extension,
{
    Invalid(ast::InvalidDocument),
    Valid(ast::Document<E>),
}

impl<E> From<RawDocumentMirror<E>> for ast::RawDocument<E>
where
    E: Extension,
{
    fn from(mirror: RawDocumentMirror<E>) -> Self {
        match mirror {
            RawDocumentMirror::Invalid(invalid) => Self::Invalid(invalid),
            RawDocumentMirror::Valid(valid) => Self::Valid(valid),
        }
    }
}

/// A `BundledDocument` as the binary form archives it: the tree and the files.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = BundledDocument)]
#[rkyv(attr(non_exhaustive))]
pub struct BundledDocumentMirror {
    #[rkyv(getter = BundledDocument::document, with = RawDocumentMirror)]
    pub document: ast::RawDocument,
    #[rkyv(getter = BundledDocument::files_by_path)]
    pub files: BTreeMap<String, Vec<u8>>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::FrontmatterFormat)]
#[rkyv(attr(non_exhaustive))]
pub enum FrontmatterFormatMirror {
    Toml,
    Yaml,
}

impl From<FrontmatterFormatMirror> for ast::FrontmatterFormat {
    fn from(mirror: FrontmatterFormatMirror) -> Self {
        match mirror {
            FrontmatterFormatMirror::Toml => Self::Toml,
            FrontmatterFormatMirror::Yaml => Self::Yaml,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Value)]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
pub enum ValueMirror {
    Boolean(bool),
    Date(#[rkyv(with = AsDays)] NaiveDate),
    DateTime(#[rkyv(with = AsDaysAndSeconds)] NaiveDateTime),
    Float(f64),
    Integer(i64),
    Mapping(#[rkyv(omit_bounds, with = MappingMirror)] ast::Mapping),
    Null(()),
    OffsetDateTime(#[rkyv(with = AsLocalAndOffset)] DateTime<FixedOffset>),
    Sequence(#[rkyv(omit_bounds, with = Map<ValueMirror>)] Vec<ast::Value>),
    String(String),
    Time(#[rkyv(with = AsSecondsOfDay)] NaiveTime),
}

impl From<ValueMirror> for ast::Value {
    fn from(mirror: ValueMirror) -> Self {
        match mirror {
            ValueMirror::Boolean(boolean) => Self::Boolean(boolean),
            ValueMirror::Date(date) => Self::Date(date),
            ValueMirror::DateTime(date_time) => Self::DateTime(date_time),
            ValueMirror::Float(float) => Self::Float(float),
            ValueMirror::Integer(integer) => Self::Integer(integer),
            ValueMirror::Mapping(mapping) => Self::Mapping(mapping),
            ValueMirror::Null(()) => Self::Null(()),
            ValueMirror::OffsetDateTime(date_time) => Self::OffsetDateTime(date_time),
            ValueMirror::Sequence(sequence) => Self::Sequence(sequence),
            ValueMirror::String(string) => Self::String(string),
            ValueMirror::Time(time) => Self::Time(time),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Block<E>)]
#[rkyv(attr(non_exhaustive))]
pub enum BlockMirror<E = Markdown>
where
    E: Extension,
{
    BlockQuote(ast::BlockQuote<E>),
    CodeBlock(ast::CodeBlock<E>),
    Html(ast::Html<E>),
    List(ast::List<E>),
    Paragraph(ast::Paragraph<E>),
    Table(ast::Table<E>),
    ThematicBreak(ast::ThematicBreak<E>),
}

impl<E> From<BlockMirror<E>> for ast::Block<E>
where
    E: Extension,
{
    fn from(mirror: BlockMirror<E>) -> Self {
        match mirror {
            BlockMirror::BlockQuote(block_quote) => Self::BlockQuote(block_quote),
            BlockMirror::CodeBlock(code_block) => Self::CodeBlock(code_block),
            BlockMirror::Html(html) => Self::Html(html),
            BlockMirror::List(list) => Self::List(list),
            BlockMirror::Paragraph(paragraph) => Self::Paragraph(paragraph),
            BlockMirror::Table(table) => Self::Table(table),
            BlockMirror::ThematicBreak(thematic_break) => Self::ThematicBreak(thematic_break),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Args)]
#[rkyv(attr(non_exhaustive))]
pub enum ArgsMirror {
    Text(String),
    Toml(#[rkyv(with = MappingMirror)] ast::Mapping),
}

impl From<ArgsMirror> for ast::Args {
    fn from(mirror: ArgsMirror) -> Self {
        match mirror {
            ArgsMirror::Text(text) => Self::Text(text),
            ArgsMirror::Toml(mapping) => Self::Toml(mapping),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Alignment)]
#[rkyv(attr(non_exhaustive))]
pub enum AlignmentMirror {
    Center,
    Default,
    Left,
    Right,
}

impl From<AlignmentMirror> for ast::Alignment {
    fn from(mirror: AlignmentMirror) -> Self {
        match mirror {
            AlignmentMirror::Center => Self::Center,
            AlignmentMirror::Default => Self::Default,
            AlignmentMirror::Left => Self::Left,
            AlignmentMirror::Right => Self::Right,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::TableFormat)]
#[rkyv(attr(non_exhaustive))]
pub enum TableFormatMirror {
    Grid,
    Pipe,
    Simple,
}

impl From<TableFormatMirror> for ast::TableFormat {
    fn from(mirror: TableFormatMirror) -> Self {
        match mirror {
            TableFormatMirror::Grid => Self::Grid,
            TableFormatMirror::Pipe => Self::Pipe,
            TableFormatMirror::Simple => Self::Simple,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Inline<E>)]
#[rkyv(attr(non_exhaustive))]
pub enum InlineMirror<E = Markdown>
where
    E: Extension,
{
    Code(ast::Code<E>),
    Emphasis(ast::Emphasis<E>),
    Html(ast::Html<E>),
    Image(ast::Image<E>),
    LineBreak(ast::LineBreak<E>),
    Link(ast::Link<E>),
    Strong(ast::Strong<E>),
    Text(ast::Text<E>),
}

impl<E> From<InlineMirror<E>> for ast::Inline<E>
where
    E: Extension,
{
    fn from(mirror: InlineMirror<E>) -> Self {
        match mirror {
            InlineMirror::Code(code) => Self::Code(code),
            InlineMirror::Emphasis(emphasis) => Self::Emphasis(emphasis),
            InlineMirror::Html(html) => Self::Html(html),
            InlineMirror::Image(image) => Self::Image(image),
            InlineMirror::LineBreak(line_break) => Self::LineBreak(line_break),
            InlineMirror::Link(link) => Self::Link(link),
            InlineMirror::Strong(strong) => Self::Strong(strong),
            InlineMirror::Text(text) => Self::Text(text),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::LinkInline<E>)]
#[rkyv(attr(non_exhaustive))]
pub enum LinkInlineMirror<E = Markdown>
where
    E: Extension,
{
    Code(ast::Code<E>),
    Emphasis(ast::LinkEmphasis<E>),
    Html(ast::Html<E>),
    Image(ast::Image<E>),
    LineBreak(ast::LineBreak<E>),
    Strong(ast::LinkStrong<E>),
    Text(ast::Text<E>),
}

impl<E> From<LinkInlineMirror<E>> for ast::LinkInline<E>
where
    E: Extension,
{
    fn from(mirror: LinkInlineMirror<E>) -> Self {
        match mirror {
            LinkInlineMirror::Code(code) => Self::Code(code),
            LinkInlineMirror::Emphasis(emphasis) => Self::Emphasis(emphasis),
            LinkInlineMirror::Html(html) => Self::Html(html),
            LinkInlineMirror::Image(image) => Self::Image(image),
            LinkInlineMirror::LineBreak(line_break) => Self::LineBreak(line_break),
            LinkInlineMirror::Strong(strong) => Self::Strong(strong),
            LinkInlineMirror::Text(text) => Self::Text(text),
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::Continues)]
#[rkyv(attr(non_exhaustive))]
pub enum ContinuesMirror {
    Both,
    Neither,
    Next,
    Previous,
}

impl From<ContinuesMirror> for ast::Continues {
    fn from(mirror: ContinuesMirror) -> Self {
        match mirror {
            ContinuesMirror::Both => Self::Both,
            ContinuesMirror::Neither => Self::Neither,
            ContinuesMirror::Next => Self::Next,
            ContinuesMirror::Previous => Self::Previous,
        }
    }
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(remote = ast::LinkForm)]
#[rkyv(attr(non_exhaustive))]
pub enum LinkFormMirror {
    Markdown,
    Wiki,
}

impl From<LinkFormMirror> for ast::LinkForm {
    fn from(mirror: LinkFormMirror) -> Self {
        match mirror {
            LinkFormMirror::Markdown => Self::Markdown,
            LinkFormMirror::Wiki => Self::Wiki,
        }
    }
}
