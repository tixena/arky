//! The document tree a markdown file parses into, and the frontmatter values it holds.
//!
//! A [`RawDocument`] is [`Valid`](RawDocument::Valid), a [`Document`], or
//! [`Invalid`](RawDocument::Invalid), an [`InvalidDocument`] whose frontmatter no entry of reads.
//! A document's body is a [`MarkdownBlock`]: its [`Section`]s, each a [`Heading`], the
//! [`Block`]s under it, then its subsections. A heading's level is not stored: it is the depth of
//! its section, counted from one in each markdown block. A [`Paragraph`] holds [`Line`]s of
//! [`Inline`] nodes. The frontmatter is a [`Mapping`] of [`Value`]s.
//!
//! Every field is public, and each node has a `new` function, or `default()` for
//! [`ThematicBreak`] and [`LineBreak`], so code can change a parsed tree or build one. Most types
//! are `#[non_exhaustive]`: build them with those functions, and give each `match` on an enum a
//! wildcard arm. A tree built by hand equals the tree its markdown parses into:
//!
//! ```rust
//! use arky::parser::ast::{
//!     Block, Document, Heading, Inline, Line, MarkdownBlock, Paragraph, RawDocument, Section,
//!     Text,
//! };
//! use arky::parser::parse;
//! use arky::writer::{WriteOptions, write};
//!
//! fn text(value: &str) -> Inline {
//!     Inline::Text(Text::new(value.to_owned()))
//! }
//!
//! let line = Line::new(vec![text("Mix, rest and bake.")]);
//! let section = Section::new(
//!     Some(Heading::new(vec![text("Bread")])),
//!     vec![Block::Paragraph(Paragraph::new(vec![line]))],
//!     Vec::new(),
//! );
//! let built = RawDocument::Valid(Document::new(MarkdownBlock::new(vec![section]), None));
//!
//! let source = "# Bread\n\nMix, rest and bake.\n";
//! assert_eq!(write(&built, &WriteOptions::default())?, source);
//! assert_eq!(parse(source).document, built);
//! # Ok::<(), arky::writer::WriteError>(())
//! ```
//!
//! Each node of the markdown tree also has a `kind` field, a [`Kind`]: the node an
//! [`Extension`] gave it, or `None`. JSON, the binary form and the stores keep no `kind`. No node
//! holds a span: an extension reads spans in the [located tree](crate::parser::located), and a
//! parse gives the spans of the frontmatter entries it left out.
//!
//! # Generated functions
//!
//! tixschema's `#[model_schema]` attribute gives the node and value types these functions, in
//! every build:
//!
//! - `ts_definition()`, the `TypeScript` type of the JSON that
//!   [`to_json`](crate::parser::to_json) writes, with the type's documentation as comments.
//! - `zod_schema()`, a Zod 4 schema of that JSON, as `TypeScript` text.
//! - `json_schema()`, a JSON Schema document of that JSON, as a `serde_json::Value`.
//! - `enum_members()`, on [`FrontmatterFormat`], [`Alignment`], [`TableFormat`], [`Continues`]
//!   and [`LinkForm`]: the name each variant is written as.
//! - `from_value_with()` and `from_value_piped()`, which read a `serde_json::Value` and hand
//!   what does not read to a callback, or to resolvers, that may repair it.
//!
//! Call a generic type with its default filled in, as `<Document>::ts_definition()`. Beside each
//! type, the attribute writes a public module named after it, such as `raw_document_schema`.
//! Its `Schema` struct gives the same descriptions, and it names the types the generated
//! functions use, such as `Issue` and `Verdict`. [`Mapping`], a type alias, has only its module,
//! `mapping_schema`.
//!
//! `from_value_with` repairs a stored document that no longer reads. Its callback gets the JSON
//! value, which it may change, and each `Issue` found in it, such as `Issue::Invalid`, which
//! holds the `path` of the value and the value `found` there. The callback returns
//! `Verdict::Fixed` to read the changed value again, or `Verdict::Reject` to fail with the
//! issues. Here a stored tag is misspelled:
//!
//! ```rust
//! use arky::parser::ast::{RawDocument, raw_document_schema};
//! use arky::parser::parse;
//! use serde_json::json;
//!
//! let stored = json!({
//!     "t": "vaild",
//!     "bd": { "se": [{ "bl": [], "hd": { "ch": [{ "t": "text", "v": "Plan" }] }, "ss": [] }] }
//! });
//! let read = <RawDocument>::from_value_with(stored, |value, issues| {
//!     for issue in issues {
//!         match issue {
//!             raw_document_schema::Issue::Invalid { path, found, .. } if *found == "vaild" => {
//!                 path.set_in_value(value, json!("valid"));
//!             }
//!             _ => {}
//!         }
//!     }
//!     raw_document_schema::Verdict::Fixed
//! })?;
//! assert_eq!(read, parse("# Plan\n").document);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! With the `mongodb` feature, each type also has `MONGO_FIELDS`, a const of typed paths that
//! build `bson` filters and updates, such as `<RawDocument>::MONGO_FIELDS.is_valid()`. It also
//! has typed operations over a `mongodb::Collection<bson::Document>`, such as `find_one` and
//! `insert_one`, and `from_bson_with` and `from_bson_piped`, which read a stored
//! `bson::Document`. An operation is awaited and fails with the schema module's
//! `OperationError`.
//!
#![cfg_attr(
    feature = "mongodb",
    doc = "
This function, which needs a running server, gives the first stored document that is
`RawDocument::Valid`:

```rust
use arky::parser::ast::RawDocument;
use arky::parser::ast::raw_document_schema::OperationError;

async fn first_valid(
    collection: &mongodb::Collection<bson::Document>,
) -> Result<Option<RawDocument>, OperationError> {
    <RawDocument>::find_one(collection, <RawDocument>::MONGO_FIELDS.is_valid()).await
}
```
"
)]
//!
//! With the `binary-format` feature, each node struct derives rkyv's `Archive`, which writes an
//! archived struct beside it, such as `ArchivedDocument`, and each enum's archived form has a
//! public name, such as `ArchivedRawDocument`. No `kind` is archived.

mod value_serde;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
#[cfg(feature = "binary-format")]
use rkyv::rancor::Source;
#[cfg(feature = "binary-format")]
use rkyv::ser::{Allocator, Writer};
#[cfg(feature = "binary-format")]
use rkyv::validation::ArchiveContext;
#[cfg(feature = "binary-format")]
use rkyv::with::{Map, Skip};
// tixschema takes every derive named `Deserialize` off a model whose read it
// writes, rkyv's with serde's.
#[cfg(feature = "binary-format")]
use rkyv::Deserialize as RkyvDeserialize;
use serde::{Deserialize, Deserializer, Serialize};
use tixschema::model_schema;

use crate::parser::extension::{Extension, Markdown, markdown_schema};
#[cfg(feature = "binary-format")]
use crate::parser::mirror::{
    self, AlignmentMirror, ArgsMirror, BlockMirror, ContinuesMirror, FrontmatterFormatMirror,
    InlineMirror, LinkFormMirror, LinkInlineMirror, MappingMirror, TableFormatMirror,
};

/// The stack left below which `grown` reads on a new segment. It holds the
/// `validate()` that tixschema runs over the subtree each grown read returns.
const RED_ZONE: usize = 3 * 1024 * 1024;

/// The size of each segment `grown` adds.
const GROWTH: usize = 4 * 1024 * 1024;

/// The node an extension gave a plain node, or `None`. Boxed, because a markdown
/// block can hold a markdown block.
///
/// [`write`](crate::writer::write) hands a claimed node to its extension, which writes it in
/// place of the plain node under it: a change to the `value` of a `markdown` code block that
/// arky's `Markdown` extension claimed does not show in the written text. JSON, the binary form
/// and the stores keep only the plain node, and
/// [`rebuild_with`](crate::parser::rebuild_with) builds the claims again.
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
    /// A claim of `node`, an extension's node, for a tree built by hand. The plain node that
    /// carries it is not changed.
    #[must_use]
    pub fn new(node: E::Node<E>) -> Self {
        Self(Some(Box::new(node)))
    }
}

/// What `parse` builds: a document whose frontmatter reads, or one whose
/// frontmatter no entry of reads.
///
/// JSON writes it with a `t` tag, `"valid"` or `"invalid"`, beside the fields of its document.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
#[serde(tag = "t", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RawDocument<E = Markdown>
where
    E: Extension,
{
    /// A file whose frontmatter keeps no entry, and refuses or drops at least one.
    Invalid(InvalidDocument),
    /// Any other file, with or without a frontmatter.
    Valid(Document<E>),
}

/// A [`RawDocument`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedRawDocument<E = Markdown> = mirror::ArchivedRawDocumentMirror<E>;

/// A document whose frontmatter no entry of reads: its body, claimed by
/// arky's markdown extension alone, and the frontmatter as written.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize),
    rkyv(attr(non_exhaustive))
)]
#[non_exhaustive]
pub struct InvalidDocument {
    /// The markdown after the frontmatter, which only arky's `Markdown` extension reads.
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<Markdown>,
    /// The frontmatter as written.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize),
    rkyv(attr(non_exhaustive))
)]
#[non_exhaustive]
pub struct InvalidFrontmatter {
    /// The reader's message for the first entry that does not read.
    #[serde(rename = "er")]
    pub error: String,
    /// The syntax the frontmatter is written in.
    #[serde(rename = "fo")]
    #[cfg_attr(feature = "binary-format", rkyv(with = FrontmatterFormatMirror))]
    pub format: FrontmatterFormat,
    /// The text between the delimiter lines, without the line ending of its last line.
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
///
/// It does not record the syntax of its frontmatter: a write picks the syntax.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Document<E = Markdown>
where
    E: Extension,
{
    /// The markdown after the frontmatter, or the whole file when it has none.
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    /// The frontmatter's entries, or `None` when the file has no frontmatter.
    #[serde(rename = "data")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "binary-format", rkyv(with = Map<MappingMirror>))]
    pub frontmatter: Option<Mapping>,
    /// The node an extension gave the whole file, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
    /// TOML, between `+++` lines.
    Toml,
    /// YAML, between `---` lines, the syntax a valid document's frontmatter is written in unless
    /// told otherwise.
    #[default]
    Yaml,
}

/// A [`FrontmatterFormat`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedFrontmatterFormat = mirror::ArchivedFrontmatterFormatMirror;

/// Keys and their values, in the order they were written. A key appears once.
///
/// In YAML, a key written twice reads as one key holding a list of its values.
#[model_schema()]
pub type Mapping = IndexMap<String, Value>;

/// What a key maps to, or what a list holds. Written untagged: a frontmatter
/// is plain values, a date a BSON `Date`, a time `{ t: "time", v: "07:32:00" }`.
///
/// JSON and BSON keep a date-time as an instant in UTC, to the millisecond: one with an offset
/// or none reads back as an `OffsetDateTime` at `+00:00`, or as a `Date` when it is at midnight
/// UTC. In JSON, a float that is not finite is the text `nan`, `inf` or `-inf`, and reads back
/// as a `String`.
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
    /// `true` or `false`.
    Boolean(bool),
    /// A whole number.
    Integer(i64),
    /// A finite float. The readers read `nan` and `inf` as a `String`.
    #[serde(with = "value_serde::finite")]
    Float(f64),
    /// Text.
    String(String),
    /// A date, such as `2026-10-17`.
    #[serde(with = "value_serde::midnight")]
    Date(NaiveDate),
    /// A date and time with an offset, such as `1979-05-27T07:32:00+02:00`.
    #[serde(with = "value_serde::instant")]
    OffsetDateTime(DateTime<FixedOffset>),
    /// A date and time with no offset, such as `1979-05-27T07:32:00`.
    #[serde(with = "value_serde::as_if_utc")]
    DateTime(NaiveDateTime),
    /// A time of day, such as `07:32:00`, which TOML reads. YAML has none: a write to YAML gives
    /// the quoted text.
    #[serde(with = "value_serde::time_of_day")]
    Time(NaiveTime),
    /// A table or a mapping: keys and their values.
    #[serde(deserialize_with = "value_serde::plain_mapping::deserialize")]
    Mapping(Mapping),
    /// A list of values.
    #[serde(deserialize_with = "grown")]
    Sequence(Vec<Self>),
    /// YAML only: TOML has no null.
    Null(()),
}

/// A [`Value`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedValue = mirror::ArchivedValueMirror;

/// A stretch of markdown read by the same rules: the body, a list item's
/// content, a table cell's content, a markdown code block's content.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct MarkdownBlock<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the block, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// Its sections, in order. Text before the first heading sits in a first section with no
    /// heading.
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
    /// Lines marked with `>`, kept as raw text.
    BlockQuote(BlockQuote<E>),
    /// A fenced or indented code block.
    CodeBlock(CodeBlock<E>),
    /// Raw HTML on lines of its own.
    Html(Html<E>),
    /// A bullet or ordered list.
    List(List<E>),
    /// Lines of text.
    Paragraph(Paragraph<E>),
    /// A pipe, grid or simple table.
    Table(Table<E>),
    /// A line such as `---`.
    ThematicBreak(ThematicBreak<E>),
}

/// A [`Block`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedBlock<E = Markdown> = mirror::ArchivedBlockMirror<E>;

/// Quoted text, without its `>` marks. Nothing inside it is read.
///
/// Parse `value` to read the markdown it holds.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct BlockQuote<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the quote, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The quoted lines, each without its `>` mark.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Html<E = Markdown>
where
    E: Extension,
{
    /// Whether an inline piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the HTML, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The HTML as written.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct ThematicBreak<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the break, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
/// A block after a subsection belongs to that subsection: a section ends at the next heading of
/// its own level or a higher one.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Section<E = Markdown>
where
    E: Extension,
{
    /// The blocks between the heading and the first subsection.
    #[serde(rename = "bl")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<BlockMirror<E>>))]
    pub blocks: Vec<Block<E>>,
    /// The heading line, or `None` when no heading line was written: for the text before the
    /// first heading, and for a level the next heading skips. A heading line with no text is a
    /// heading with no children.
    #[serde(rename = "hd")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<Heading<E>>,
    /// The node an extension gave the section, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The sections one level deeper, in order.
    #[serde(rename = "ss")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds))]
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

/// A heading line, such as `# Bread`, or a line of text underlined with `===` or `---`. An empty
/// heading has no children.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Heading<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes of the heading's text.
    #[serde(rename = "ch")]
    #[cfg_attr(feature = "binary-format", rkyv(with = Map<InlineMirror<E>>))]
    pub children: Vec<Inline<E>>,
    /// The node an extension gave the heading, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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

/// Lines of text, ended by a blank line or by a block that starts on the next line, such as a
/// list item, a heading or a fence.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Paragraph<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the paragraph, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// Its lines, in order.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Line<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes written on the line.
    #[serde(rename = "ch")]
    #[cfg_attr(feature = "binary-format", rkyv(with = Map<InlineMirror<E>>))]
    pub children: Vec<Inline<E>>,
    /// The node an extension gave the line, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
/// their number.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct List<E = Markdown>
where
    E: Extension,
{
    /// Its items, in order.
    #[serde(rename = "it")]
    pub items: Vec<Item<E>>,
    /// The node an extension gave the list, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// Whether a blank line separates two items, or two blocks of one item. A loose list is
    /// written with a blank line between its items.
    #[serde(rename = "lo")]
    pub loose: bool,
    /// An ordered list's first number, or `None` for a bullet list.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Item<E = Markdown>
where
    E: Extension,
{
    /// The markdown the item holds, its heading levels counted from one.
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    /// The node an extension gave the item, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
/// The info string, the text after the opening fence, is split into `language`, its first
/// word, and `args`, the rest. arky's `Markdown` extension reads a block whose language is
/// `markdown` or `md` as a whole document.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct CodeBlock<E = Markdown>
where
    E: Extension,
{
    /// The rest of the info string, after the language.
    #[serde(rename = "ar")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ArgsMirror))]
    pub args: Args,
    /// The node an extension gave the block, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The first word of the info string, or `None` when there is no info string.
    #[serde(rename = "la")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The lines of the block, without the fences or the indentation, and with no line ending
    /// after the last.
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
///
/// `title="README.md",showLineNumbers=true` reads as `Toml`, and `Example of a list` as `Text`.
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

/// An [`Args`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedArgs = mirror::ArchivedArgsMirror;

/// A table read from pipe, grid or simple table syntax. `head` holds the
/// header rows, `rows` the rest.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Table<E = Markdown>
where
    E: Extension,
{
    /// How each column's cells are aligned, one per column.
    #[serde(rename = "al")]
    #[cfg_attr(feature = "binary-format", rkyv(with = Map<AlignmentMirror>))]
    pub alignments: Vec<Alignment>,
    /// The syntax the table was read from. A write picks a pipe or a grid table by the table's
    /// shape, whatever this says.
    #[serde(rename = "fo")]
    #[cfg_attr(feature = "binary-format", rkyv(with = TableFormatMirror))]
    pub format: TableFormat,
    /// The header rows.
    #[serde(rename = "he")]
    pub head: Vec<TableRow<E>>,
    /// The node an extension gave the table, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The rows under the header.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct TableRow<E = Markdown>
where
    E: Extension,
{
    /// Its cells, one per column.
    #[serde(rename = "ce")]
    pub cells: Vec<TableCell<E>>,
    /// The node an extension gave the row, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct TableCell<E = Markdown>
where
    E: Extension,
{
    /// The markdown the cell holds, its heading levels counted from one.
    #[serde(rename = "bd")]
    pub body: MarkdownBlock<E>,
    /// The node an extension gave the cell, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
    /// Centered: `:---:` in a pipe table's delimiter row.
    Center,
    /// No alignment: `---`.
    Default,
    /// Left: `:---`.
    Left,
    /// Right: `---:`.
    Right,
}

/// An [`Alignment`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
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

/// A [`TableFormat`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
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
    /// Text between backticks.
    Code(Code<E>),
    /// Text between `*` or `_`.
    Emphasis(Emphasis<E>),
    /// Inline HTML, such as `<span>`.
    Html(Html<E>),
    /// An embed, such as `![alt](target)`.
    Image(Image<E>),
    /// A hard line break.
    LineBreak(LineBreak<E>),
    /// A link, such as `[text](target)`.
    Link(Link<E>),
    /// Text between `**` or `__`.
    Strong(Strong<E>),
    /// Plain text.
    Text(Text<E>),
}

/// An [`Inline`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
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

/// A [`Continues`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedContinues = mirror::ArchivedContinuesMirror;

/// Plain text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Text<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the text, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The text, its escapes and character references resolved.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Emphasis<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes between the marks.
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<InlineMirror<E>>))]
    pub children: Vec<Inline<E>>,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the emphasis, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Strong<E = Markdown>
where
    E: Extension,
{
    /// The inline nodes between the marks.
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<InlineMirror<E>>))]
    pub children: Vec<Inline<E>>,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the strong emphasis, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct LineBreak<E = Markdown>
where
    E: Extension,
{
    /// The node an extension gave the line break, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Link<E = Markdown>
where
    E: Extension,
{
    /// The text shown, empty for `[[target]]`.
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>))]
    pub children: Vec<LinkInline<E>>,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// How the link is written: `[text](target)` or `[[target|text]]`.
    #[serde(rename = "fr")]
    #[cfg_attr(feature = "binary-format", rkyv(with = LinkFormMirror))]
    pub form: LinkForm,
    /// The node an extension gave the link, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// Where the link goes.
    #[serde(rename = "tg")]
    pub target: String,
    /// The title written after the target, if any.
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
    /// Text between backticks.
    Code(Code<E>),
    /// Text between `*` or `_`.
    Emphasis(LinkEmphasis<E>),
    /// Inline HTML.
    Html(Html<E>),
    /// An embed, such as a badge inside a link.
    Image(Image<E>),
    /// A hard line break.
    LineBreak(LineBreak<E>),
    /// Text between `**` or `__`.
    Strong(LinkStrong<E>),
    /// Plain text.
    Text(Text<E>),
}

/// A [`LinkInline`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedLinkInline<E = Markdown> = mirror::ArchivedLinkInlineMirror<E>;

/// Text between `*` or `_` inside a link's text.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct LinkEmphasis<E = Markdown>
where
    E: Extension,
{
    /// The nodes between the marks.
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>))]
    pub children: Vec<LinkInline<E>>,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the emphasis, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(serialize_bounds(__S: Writer + Allocator, __S::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(deserialize_bounds(__D::Error: Source)))]
#[cfg_attr(feature = "binary-format", rkyv(bytecheck(bounds(__C: ArchiveContext, __C::Error: Source))))]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct LinkStrong<E = Markdown>
where
    E: Extension,
{
    /// The nodes between the marks.
    #[serde(rename = "ch")]
    #[serde(deserialize_with = "grown")]
    #[cfg_attr(feature = "binary-format", rkyv(omit_bounds, with = Map<LinkInlineMirror<E>>))]
    pub children: Vec<LinkInline<E>>,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the strong emphasis, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
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

/// A [`LinkForm`] as rkyv archives it, with the `binary-format` feature.
#[cfg(feature = "binary-format")]
pub type ArchivedLinkForm = mirror::ArchivedLinkFormMirror;

/// An embed: `![alt](target "title")`, `![[target]]` or `![[target|alt]]`.
///
/// `alt` is empty when none is written. `target` and `title` hold their
/// escapes and references resolved.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Image<E = Markdown>
where
    E: Extension,
{
    /// The text in brackets, or the wiki alias after `|`.
    #[serde(rename = "at")]
    pub alt: String,
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// How the embed is written: `![alt](target)` or `![[target|alt]]`.
    #[serde(rename = "fr")]
    #[cfg_attr(feature = "binary-format", rkyv(with = LinkFormMirror))]
    pub form: LinkForm,
    /// The node an extension gave the image, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The file or address the embed names.
    #[serde(rename = "tg")]
    pub target: String,
    /// The title written after the target, if any.
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

/// Text between backticks, such as `` `cargo test` ``.
#[model_schema(default_types(E = Markdown), enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "binary-format",
    derive(rkyv::Archive, rkyv::Serialize, RkyvDeserialize)
)]
#[serde(bound = "")]
#[cfg_attr(feature = "binary-format", rkyv(attr(non_exhaustive)))]
#[non_exhaustive]
pub struct Code<E = Markdown>
where
    E: Extension,
{
    /// Whether this piece goes on across a line end.
    #[serde(rename = "co")]
    #[cfg_attr(feature = "binary-format", rkyv(with = ContinuesMirror))]
    pub continues: Continues,
    /// The node an extension gave the code span, or none.
    #[serde(skip)]
    #[cfg_attr(feature = "binary-format", rkyv(with = Skip))]
    pub kind: Kind<E>,
    /// The text between the backticks.
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
