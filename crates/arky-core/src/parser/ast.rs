//! The document tree a markdown file parses into.
//!
//! Every node carries `kind`, the type an extension gave it, and none holds
//! a span: a parse reports positions beside the document.

mod value_serde;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
use rkyv::rancor::Source;
use rkyv::ser::{Allocator, Writer};
use rkyv::validation::ArchiveContext;
use rkyv::with::{Map, Skip};
// tixschema takes every derive named `Deserialize` off a model whose read it
// writes, rkyv's with serde's.
use rkyv::Deserialize as RkyvDeserialize;
use serde::{Deserialize, Deserializer, Serialize};
use tixschema::model_schema;

use crate::parser::extension::{Extension, Markdown, markdown_schema};
use crate::parser::mirror::{
    self, AlignmentMirror, ArgsMirror, BlockMirror, ContinuesMirror, FrontmatterFormatMirror,
    InlineMirror, LinkFormMirror, LinkInlineMirror, MappingMirror, TableFormatMirror,
};

/// The stack left below which `grown` reads on a new segment. It holds the
/// `validate()` that tixschema runs over the subtree each grown read returns.
const RED_ZONE: usize = 3 * 1024 * 1024;

/// The size of each segment `grown` adds.
const GROWTH: usize = 4 * 1024 * 1024;

/// The type an extension gave a node, or `None`. Boxed, because a markdown
/// block can hold a markdown block.
///
/// JSON leaves it out: the claim pass builds it again from the plain node
/// under it. Editing it leaves that plain node as it was, so a store writes
/// each claimed node back into its plain node before it writes the tree.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Kind<E>(pub Option<Box<E::Node<E>>>)
where
    E: Extension;

impl<E> Default for Kind<E>
where
    E: Extension,
{
    fn default() -> Self {
        Self(None)
    }
}

impl<E> Kind<E>
where
    E: Extension,
{
    /// `node`, an extension's node. The plain node that carries it is not
    /// changed, and JSON, binary and the stores keep only that plain node.
    #[must_use]
    pub fn new(node: E::Node<E>) -> Self {
        Self(Some(Box::new(node)))
    }
}

/// What `parse` builds: a document whose frontmatter reads, or one whose
/// frontmatter no entry of reads.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
#[serde(tag = "t", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RawDocument<E = Markdown>
where
    E: Extension,
{
    Invalid(InvalidDocument),
    Valid(Document<E>),
}

/// A `RawDocument` as rkyv archives it.
pub type ArchivedRawDocument<E = Markdown> = mirror::ArchivedRawDocumentMirror<E>;

/// A document whose frontmatter no entry of reads: its body, claimed by
/// arky's markdown extension alone, and the frontmatter as written.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct InvalidDocument {
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<Markdown>,
    #[serde(rename = "data")]
    pub frontmatter: InvalidFrontmatter,
}

impl InvalidDocument {
    /// A document of `body` whose `frontmatter` no entry of reads.
    #[must_use]
    pub const fn new(body: MarkdownBlock<Markdown>, frontmatter: InvalidFrontmatter) -> Self {
        Self { body, frontmatter }
    }
}

/// A frontmatter no entry of reads: the reader's message, its syntax, and its
/// text as written, so that a write gives it back.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    RkyvDeserialize,
)]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct InvalidFrontmatter {
    #[serde(rename = "er")]
    pub error: String,
    #[serde(rename = "fo")]
    #[rkyv(with = FrontmatterFormatMirror)]
    pub format: FrontmatterFormat,
    #[serde(rename = "tx")]
    pub text: String,
}

impl InvalidFrontmatter {
    /// A frontmatter in `format`, written as `text`, that no entry of reads, and
    /// `error`, the reader's message.
    #[must_use]
    pub const fn new(format: FrontmatterFormat, text: String, error: String) -> Self {
        Self {
            error,
            format,
            text,
        }
    }
}

/// A markdown file whose frontmatter, if it has one, reads.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Document<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    #[serde(rename = "data")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[rkyv(with = Map<MappingMirror>)]
    pub frontmatter: Option<Mapping>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Document<E>
where
    E: Extension,
{
    /// A document of `body` and `frontmatter`, with no extension node.
    #[must_use]
    pub const fn new(body: MarkdownBlock<E>, frontmatter: Option<Mapping>) -> Self {
        Self {
            body,
            frontmatter,
            kind: Kind(None),
        }
    }
}

/// The syntax a frontmatter block is written in.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FrontmatterFormat {
    Toml,
    #[default]
    Yaml,
}

/// A `FrontmatterFormat` as rkyv archives it.
pub type ArchivedFrontmatterFormat = mirror::ArchivedFrontmatterFormatMirror;

/// Keys and their values, in the order they were written. A key appears once.
#[model_schema()]
pub type Mapping = IndexMap<String, Value>;

/// What a key maps to, or what a list holds. Written untagged: a frontmatter
/// is plain values, a date a BSON `Date`, a time `{ t: "time", v: "07:32:00" }`.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
// WARNING: the variant order is the read order, so it is not alphabetical.
// serde reads an untagged enum by trying its variants top to bottom and keeps
// the first that reads. `Integer` comes before `Float`, so `2` reads as an
// integer. `Date` comes before `OffsetDateTime`, so a date at midnight UTC
// reads as a date. The dates come before `Time`, and `Time` before `Mapping`,
// so a `$date` or `{ t, v }` document is not read as a mapping. Sorting these
// variants changes what a stored value reads back as.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "serde reads an untagged enum's variants in declaration order"
)]
pub enum Value {
    Boolean(bool),
    Integer(i64),
    /// A finite float.
    #[serde(with = "value_serde::finite")]
    Float(f64),
    String(String),
    #[serde(with = "value_serde::midnight")]
    Date(NaiveDate),
    /// A date and time with an offset, such as `1979-05-27T07:32:00+02:00`.
    #[serde(with = "value_serde::instant")]
    OffsetDateTime(DateTime<FixedOffset>),
    /// A date and time with no offset, such as `1979-05-27T07:32:00`.
    #[serde(with = "value_serde::as_if_utc")]
    DateTime(NaiveDateTime),
    #[serde(with = "value_serde::time_of_day")]
    Time(NaiveTime),
    #[serde(deserialize_with = "value_serde::plain_mapping::deserialize")]
    Mapping(Mapping),
    #[serde(deserialize_with = "grown")]
    Sequence(Vec<Self>),
    /// YAML only: TOML has no null.
    Null(()),
}

/// A `Value` as rkyv archives it.
pub type ArchivedValue = mirror::ArchivedValueMirror;

/// A stretch of markdown read by the same rules: the body, a list item's
/// content, a table cell's content, a markdown code block's content.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct MarkdownBlock<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "se")]
    #[serde(deserialize_with = "grown")]
    pub sections: Vec<Section<E>>,
}

impl<E> MarkdownBlock<E>
where
    E: Extension,
{
    /// A markdown block of `sections`, with no extension node.
    #[must_use]
    pub const fn new(sections: Vec<Section<E>>) -> Self {
        Self {
            kind: Kind(None),
            sections,
        }
    }
}

/// A node that stands on its own lines.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
#[serde(tag = "t", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Block<E = Markdown>
where
    E: Extension,
{
    BlockQuote(BlockQuote<E>),
    CodeBlock(CodeBlock<E>),
    Html(Html<E>),
    List(List<E>),
    Paragraph(Paragraph<E>),
    Table(Table<E>),
    ThematicBreak(ThematicBreak<E>),
}

/// A `Block` as rkyv archives it.
pub type ArchivedBlock<E = Markdown> = mirror::ArchivedBlockMirror<E>;

/// Quoted text, without its `>` marks. Nothing inside it is read.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct BlockQuote<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "v")]
    pub value: String,
}

impl<E> BlockQuote<E>
where
    E: Extension,
{
    /// A block quote of `value`, with no extension node.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self {
            kind: Kind(None),
            value,
        }
    }
}

/// Raw HTML, as written: a block, or an inline node such as `<span>`. A
/// block's `continues` is always `Continues::Neither`.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Html<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "v")]
    pub value: String,
}

impl<E> Html<E>
where
    E: Extension,
{
    /// Raw HTML `value`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self {
            continues: Continues::Neither,
            kind: Kind(None),
            value,
        }
    }
}

/// A line of three or more `-`, `*` or `_`, such as `---`.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct ThematicBreak<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Default for ThematicBreak<E>
where
    E: Extension,
{
    fn default() -> Self {
        Self { kind: Kind(None) }
    }
}

/// A heading, the blocks under it, then its subsections. Its level is its
/// depth in the `MarkdownBlock`.
///
/// `heading` is `None` when no heading line was written, and a heading line
/// with no text is a heading with no children.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Section<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "bl")]
    #[rkyv(omit_bounds, with = Map<BlockMirror<E>>)]
    pub blocks: Vec<Block<E>>,
    #[serde(rename = "hd")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<Heading<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "ss")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds)]
    pub subsections: Vec<Self>,
}

impl<E> Section<E>
where
    E: Extension,
{
    /// A section of `heading`, the `blocks` under it and its `subsections`, with
    /// no extension node.
    #[must_use]
    pub const fn new(
        heading: Option<Heading<E>>,
        blocks: Vec<Block<E>>,
        subsections: Vec<Self>,
    ) -> Self {
        Self {
            blocks,
            heading,
            kind: Kind(None),
            subsections,
        }
    }
}

/// A heading line, such as `# Ship parser`. An empty heading has no children.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Heading<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[rkyv(with = Map<InlineMirror<E>>)]
    pub children: Vec<Inline<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Heading<E>
where
    E: Extension,
{
    /// A heading line of `children`, with no extension node.
    #[must_use]
    pub const fn new(children: Vec<Inline<E>>) -> Self {
        Self {
            children,
            kind: Kind(None),
        }
    }
}

/// A set of lines, ended by a blank line.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Paragraph<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "li")]
    pub lines: Vec<Line<E>>,
}

impl<E> Paragraph<E>
where
    E: Extension,
{
    /// A paragraph of `lines`, with no extension node.
    #[must_use]
    pub const fn new(lines: Vec<Line<E>>) -> Self {
        Self {
            kind: Kind(None),
            lines,
        }
    }
}

/// One line of a paragraph and the inline nodes written on it.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Line<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[rkyv(with = Map<InlineMirror<E>>)]
    pub children: Vec<Inline<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Line<E>
where
    E: Extension,
{
    /// A line of `children`, with no extension node.
    #[must_use]
    pub const fn new(children: Vec<Inline<E>>) -> Self {
        Self {
            children,
            kind: Kind(None),
        }
    }
}

/// A bullet or ordered list.
///
/// Its items have the same bullet character, or the same delimiter after
/// their number. `start` is an ordered list's first number, `None` for a
/// bullet list. `loose` is set when a blank line separates two items, or two
/// blocks of one item.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct List<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "it")]
    pub items: Vec<Item<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "lo")]
    pub loose: bool,
    #[serde(rename = "st")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
}

impl<E> List<E>
where
    E: Extension,
{
    /// A list of `items`, loose when `loose` is set, ordered from `start` or a
    /// bullet list when `start` is `None`, with no extension node.
    #[must_use]
    pub const fn new(items: Vec<Item<E>>, loose: bool, start: Option<u64>) -> Self {
        Self {
            items,
            kind: Kind(None),
            loose,
            start,
        }
    }
}

/// One entry of a list.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Item<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Item<E>
where
    E: Extension,
{
    /// A list item of `body`, with no extension node.
    #[must_use]
    pub const fn new(body: MarkdownBlock<E>) -> Self {
        Self {
            body,
            kind: Kind(None),
        }
    }
}

/// Lines between two fences of backticks or of tildes, or indented four
/// columns past their parent's.
///
/// `value` holds the lines as the block holds them. The info string, the
/// text after the opening fence, is split into `language`, its first word,
/// `None` when there is no info string, and `args`, the rest.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct CodeBlock<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ar")]
    #[rkyv(with = ArgsMirror)]
    pub args: Args,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "la")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(rename = "v")]
    pub value: String,
}

impl<E> CodeBlock<E>
where
    E: Extension,
{
    /// A code block of `value` whose info string is `language` and `args`, with
    /// no extension node.
    #[must_use]
    pub const fn new(language: Option<String>, args: Args, value: String) -> Self {
        Self {
            args,
            kind: Kind(None),
            language,
            value,
        }
    }
}

/// The rest of a fenced code block's info string, after its language.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Args {
    /// The rest as written, when it is not the inside of a TOML inline table.
    Text(String),
    /// The rest read as the inside of a TOML inline table; empty when the
    /// fence has nothing after its language.
    Toml(Mapping),
}

/// An `Args` as rkyv archives it.
pub type ArchivedArgs = mirror::ArchivedArgsMirror;

/// A table read from pipe, grid or simple table syntax. `head` holds the
/// header rows, `rows` the rest.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Table<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "al")]
    #[rkyv(with = Map<AlignmentMirror>)]
    pub alignments: Vec<Alignment>,
    #[serde(rename = "fo")]
    #[rkyv(with = TableFormatMirror)]
    pub format: TableFormat,
    #[serde(rename = "he")]
    pub head: Vec<TableRow<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "ro")]
    pub rows: Vec<TableRow<E>>,
}

impl<E> Table<E>
where
    E: Extension,
{
    /// A table written in `format`, its columns aligned by `alignments`, of the
    /// header rows `head` and the other rows `rows`, with no extension node.
    #[must_use]
    pub const fn new(
        format: TableFormat,
        alignments: Vec<Alignment>,
        head: Vec<TableRow<E>>,
        rows: Vec<TableRow<E>>,
    ) -> Self {
        Self {
            alignments,
            format,
            head,
            kind: Kind(None),
            rows,
        }
    }
}

/// One row of a table.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct TableRow<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ce")]
    pub cells: Vec<TableCell<E>>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> TableRow<E>
where
    E: Extension,
{
    /// A table row of `cells`, with no extension node.
    #[must_use]
    pub const fn new(cells: Vec<TableCell<E>>) -> Self {
        Self {
            cells,
            kind: Kind(None),
        }
    }
}

/// One cell of a table row.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct TableCell<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> TableCell<E>
where
    E: Extension,
{
    /// A table cell of `body`, with no extension node.
    #[must_use]
    pub const fn new(body: MarkdownBlock<E>) -> Self {
        Self {
            body,
            kind: Kind(None),
        }
    }
}

/// How a column's cells are aligned.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Alignment {
    Center,
    Default,
    Left,
    Right,
}

/// An `Alignment` as rkyv archives it.
pub type ArchivedAlignment = mirror::ArchivedAlignmentMirror;

/// The syntax a table is written in.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TableFormat {
    /// `+---+` borders and `|` walls.
    Grid,
    /// `| a | b |` rows under a `|---|---|` delimiter row.
    Pipe,
    /// Columns marked by groups of dashes under a header line.
    Simple,
}

/// A `TableFormat` as rkyv archives it.
pub type ArchivedTableFormat = mirror::ArchivedTableFormatMirror;

/// A node inside a line of text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
#[serde(tag = "t", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Inline<E = Markdown>
where
    E: Extension,
{
    Code(Code<E>),
    Emphasis(Emphasis<E>),
    Html(Html<E>),
    Image(Image<E>),
    LineBreak(LineBreak<E>),
    Link(Link<E>),
    Strong(Strong<E>),
    Text(Text<E>),
}

/// An `Inline` as rkyv archives it.
pub type ArchivedInline<E = Markdown> = mirror::ArchivedInlineMirror<E>;

/// Whether an inline node goes on from the line before or into the line
/// after. A node written across lines of a paragraph is cut into one piece
/// per line.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Continues {
    /// A middle piece: it goes on from the line before and into the line after.
    Both,
    /// The node is whole on its line.
    #[default]
    Neither,
    /// The first piece: it goes on into the line after.
    Next,
    /// The last piece: it goes on from the line before.
    Previous,
}

/// A `Continues` as rkyv archives it.
pub type ArchivedContinues = mirror::ArchivedContinuesMirror;

/// Plain text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Text<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "v")]
    pub value: String,
}

impl<E> Text<E>
where
    E: Extension,
{
    /// Plain text `value`, with no extension node.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self {
            kind: Kind(None),
            value,
        }
    }
}

/// Text between `*` or `_`, such as `*a*`.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Emphasis<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds, with = Map<InlineMirror<E>>)]
    pub children: Vec<Inline<E>>,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Emphasis<E>
where
    E: Extension,
{
    /// An emphasis of `children`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(children: Vec<Inline<E>>) -> Self {
        Self {
            children,
            continues: Continues::Neither,
            kind: Kind(None),
        }
    }
}

/// Text between `**` or `__`, such as `**b**`.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Strong<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds, with = Map<InlineMirror<E>>)]
    pub children: Vec<Inline<E>>,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Strong<E>
where
    E: Extension,
{
    /// A strong emphasis of `children`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(children: Vec<Inline<E>>) -> Self {
        Self {
            children,
            continues: Continues::Neither,
            kind: Kind(None),
        }
    }
}

/// A hard line break: the two or more spaces, or the backslash, that end a
/// line followed by another line of its paragraph.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct LineBreak<E = Markdown>
where
    E: Extension,
{
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> Default for LineBreak<E>
where
    E: Extension,
{
    fn default() -> Self {
        Self { kind: Kind(None) }
    }
}

/// A link: `[text](target "title")`, a reference link, an autolink,
/// `[[target]]` or `[[target|text]]`.
///
/// `children` are the text shown, and are empty for `[[target]]`. A
/// reference link takes its target and title from its label's definition.
/// `target` and `title` hold their escapes and references resolved.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Link<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>)]
    pub children: Vec<LinkInline<E>>,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(rename = "fr")]
    #[rkyv(with = LinkFormMirror)]
    pub form: LinkForm,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "tg")]
    pub target: String,
    #[serde(rename = "ti")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl<E> Link<E>
where
    E: Extension,
{
    /// A link written in `form` to `target`, titled `title`, whose text is
    /// `children`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(
        form: LinkForm,
        target: String,
        title: Option<String>,
        children: Vec<LinkInline<E>>,
    ) -> Self {
        Self {
            children,
            continues: Continues::Neither,
            form,
            kind: Kind(None),
            target,
            title,
        }
    }
}

/// A node inside a link's text: any inline node but a link, at any depth.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
#[serde(tag = "t", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum LinkInline<E = Markdown>
where
    E: Extension,
{
    Code(Code<E>),
    Emphasis(LinkEmphasis<E>),
    Html(Html<E>),
    Image(Image<E>),
    LineBreak(LineBreak<E>),
    Strong(LinkStrong<E>),
    Text(Text<E>),
}

/// A `LinkInline` as rkyv archives it.
pub type ArchivedLinkInline<E = Markdown> = mirror::ArchivedLinkInlineMirror<E>;

/// Text between `*` or `_` inside a link's text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct LinkEmphasis<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>)]
    pub children: Vec<LinkInline<E>>,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> LinkEmphasis<E>
where
    E: Extension,
{
    /// An emphasis inside a link's text, of `children`, whole on its line, with
    /// no extension node.
    #[must_use]
    pub const fn new(children: Vec<LinkInline<E>>) -> Self {
        Self {
            children,
            continues: Continues::Neither,
            kind: Kind(None),
        }
    }
}

/// Text between `**` or `__` inside a link's text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source))]
#[rkyv(deserialize_bounds(__D::Error: Source))]
#[rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source)))]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct LinkStrong<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>)]
    pub children: Vec<LinkInline<E>>,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
}

impl<E> LinkStrong<E>
where
    E: Extension,
{
    /// A strong emphasis inside a link's text, of `children`, whole on its line,
    /// with no extension node.
    #[must_use]
    pub const fn new(children: Vec<LinkInline<E>>) -> Self {
        Self {
            children,
            continues: Continues::Neither,
            kind: Kind(None),
        }
    }
}

/// How a link or an image is written.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum LinkForm {
    /// `[text](target)`, or `![alt](target)` for an image.
    Markdown,
    /// `[[target|text]]`, or `![[target|alt]]` for an image.
    Wiki,
}

/// A `LinkForm` as rkyv archives it.
pub type ArchivedLinkForm = mirror::ArchivedLinkFormMirror;

/// An embed: `![alt](target "title")`, `![[target]]` or `![[target|alt]]`.
///
/// `alt` is empty when none is written. `target` and `title` hold their
/// escapes and references resolved.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Image<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "at")]
    pub alt: String,
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(rename = "fr")]
    #[rkyv(with = LinkFormMirror)]
    pub form: LinkForm,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "tg")]
    pub target: String,
    #[serde(rename = "ti")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl<E> Image<E>
where
    E: Extension,
{
    /// An image written in `form` of `target`, titled `title`, whose alternative
    /// text is `alt`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(form: LinkForm, target: String, title: Option<String>, alt: String) -> Self {
        Self {
            alt,
            continues: Continues::Neither,
            form,
            kind: Kind(None),
            target,
            title,
        }
    }
}

/// Text between backticks, such as `` `arky check` ``.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize, RkyvDeserialize,
)]
#[serde(bound = "")]
#[rkyv(attr(non_exhaustive))]
#[non_exhaustive]
pub struct Code<E = Markdown>
where
    E: Extension,
{
    #[serde(rename = "co")]
    #[rkyv(with = ContinuesMirror)]
    pub continues: Continues,
    #[serde(skip)]
    #[rkyv(with = Skip)]
    pub kind: Kind<E>,
    #[serde(rename = "v")]
    pub value: String,
}

impl<E> Code<E>
where
    E: Extension,
{
    /// Code `value`, whole on its line, with no extension node.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self {
            continues: Continues::Neither,
            kind: Kind(None),
            value,
        }
    }
}

/// Reads a field with the stack grown when little is left: serde reads an
/// internally tagged enum from a buffer that `serde_stacker` does not guard.
fn grown<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    stacker::maybe_grow(RED_ZONE, GROWTH, || T::deserialize(deserializer))
}
