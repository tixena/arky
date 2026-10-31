//! arky reads a markdown file, frontmatter included, into a typed tree and writes the tree back
//! as markdown. Code can read, change or build that tree in between.
//!
//! The same tree also travels as JSON, as a zip or a single markdown file that holds the images
//! it embeds, and as a binary archive. Its types publish their `TypeScript`, Zod and JSON Schema
//! descriptions.
//!
//! # The problem and who it is for
//!
//! A program that keeps notes, recipes, tasks or blog posts as markdown files needs to read a
//! frontmatter value, change a list or a table, and save the file. arky gives the file as Rust
//! data: the frontmatter as typed values (integers, floats, dates, lists, mappings) and the body
//! as a tree of sections, blocks and inline nodes, each a type with public fields. It writes any
//! tree, parsed or built, in one form for each construct, so a file written once stays the same
//! on every later write.
//!
//! Use it in note and static-site tools, in scripts and coding agents that edit markdown through
//! types, in services that keep markdown as JSON, and in tools that move a document together with
//! its pictures.
//!
//! # What is in the crate
//!
//! | Module | What it holds | Feature |
//! | --- | --- | --- |
//! | [`parser`] | [`parse`](parser::parse) and [`parse_with`](parser::parse_with), from text to tree; [`to_json`](parser::to_json) and [`from_json`](parser::from_json); [`read_toml`](parser::read_toml) and [`read_yaml`](parser::read_yaml); [`ParseOptions`](parser::ParseOptions) | none |
//! | [`parser::ast`] | The tree: [`RawDocument`](parser::ast::RawDocument), [`Document`](parser::ast::Document), [`MarkdownBlock`](parser::ast::MarkdownBlock), [`Section`](parser::ast::Section), [`Block`](parser::ast::Block), [`Inline`](parser::ast::Inline), and the frontmatter's [`Mapping`](parser::ast::Mapping) of [`Value`](parser::ast::Value). The node and value types have `ts_definition`, `zod_schema` and `json_schema` | none; `MONGO_FIELDS` and typed `MongoDB` operations with `mongodb`; the archived types with `binary-format` |
//! | [`parser::extension`] | The [`Extension`](parser::extension::Extension) trait, which turns the constructs it claims into node types of your own, and arky's [`Markdown`](parser::extension::Markdown) extension, which reads a `markdown` or `md` code block as a whole document | none |
//! | [`parser::located`] | The tree with a byte span on every node, as `Extension::claim` receives it | none |
//! | [`parser::error`] | [`ParseError`](parser::error::ParseError), a message and a byte span, and [`RebuildError`](parser::error::RebuildError) | none |
//! | `parser::archive` | rkyv adapters for the chrono types a frontmatter value holds | `binary-format` |
//! | [`writer`] | [`write`](writer::write), [`WriteOptions`](writer::WriteOptions), and the [`Writer`](writer::Writer) an extension writes its nodes with | none; `write_archived` with `binary-format` |
//! | [`value`] | [`to_value`](value::to_value) and [`from_value`](value::from_value), between frontmatter values and your own serde types | none |
//! | [`bundle`] | [`BundledDocument`](bundle::BundledDocument): a document and every file its image embeds name, read from and written to an os-shim `System` or a zip. [`embeds`](bundle::embeds), [`embedded_paths`](bundle::embedded_paths) and [`path_of`](bundle::path_of), which say what a document embeds and where each file is; [`images_mut`](bundle::images_mut), which changes every image target; [`media_type`](bundle::media_type) | none |
//! | [`forms`] | [`encode`](forms::encode), [`decode`](forms::decode) and [`stored`](forms::stored) over each [`Form`](forms::Form): markdown, JSON and zip | none; the binary form with `binary-format` |
//!
//! # Getting started
//!
//! Add the crate. With the default features, arky parses, edits and
//! writes documents, and gives the markdown, JSON and zip forms.
//!
//! ```toml
//! [dependencies]
//! arky = "0.2"
//! ```
//!
//! Parse a file, read a frontmatter value, change it, and write the file back:
//!
//! ```rust
//! use arky::parser::ast::{RawDocument, Value};
//! use arky::parser::parse;
//! use arky::writer::{WriteOptions, write};
//!
//! let source = "---\ntitle: Bread\nservings: 2\n---\n# Bread\n\nMix, rest and bake.\n";
//! let mut parsed = parse(source);
//! assert!(parsed.dropped.is_empty());
//!
//! let RawDocument::Valid(document) = &mut parsed.document else {
//!     panic!("every frontmatter entry reads");
//! };
//! let frontmatter = document.frontmatter.as_mut().expect("the file has a frontmatter");
//! assert_eq!(frontmatter.get("servings"), Some(&Value::Integer(2)));
//! frontmatter.insert("servings".to_owned(), Value::Integer(4));
//!
//! let text = write(&parsed.document, &WriteOptions::default())?;
//! assert_eq!(text, "---\ntitle: Bread\nservings: 4\n---\n# Bread\n\nMix, rest and bake.\n");
//! # Ok::<(), arky::writer::WriteError>(())
//! ```
//!
//! [`parse`](parser::parse) returns no error. A frontmatter entry that does not read is left out
//! and listed, with its byte span, in [`Parsed::dropped`](parser::Parsed::dropped). When no entry
//! of the frontmatter reads, the result is
//! [`RawDocument::Invalid`](parser::ast::RawDocument::Invalid), which keeps the frontmatter's
//! text.
//!
//! Some calls take types from other crates. Add `os-shim` for
//! [`BundledDocument::from_file`](bundle::BundledDocument::from_file) and
//! [`BundledDocument::to_file`](bundle::BundledDocument::to_file), `chrono` to build date and time
//! values, and `serde` to read a frontmatter into a type of your own with
//! [`value::from_value`].
//!
//! # Use cases
//!
//! ## Edit a document and keep the file stable
//!
//! A script adds an item to a shopping list and saves the note. Nodes have `new` functions and
//! public fields, so the change is a push onto a `Vec`. The written text is arky's form:
//! `*` bullets become `-`, and a second write gives the same text.
//!
//! ```rust
//! use arky::parser::ast::{
//!     Block, Inline, Item, Line, MarkdownBlock, Paragraph, RawDocument, Section, Text,
//! };
//! use arky::parser::parse;
//! use arky::writer::{WriteOptions, write};
//!
//! let mut parsed = parse("# Groceries\n\n* flour\n* yeast\n");
//! let RawDocument::Valid(document) = &mut parsed.document else {
//!     panic!("a file with no frontmatter is valid");
//! };
//! let groceries = &mut document.body.sections[0];
//! let Some(Block::List(list)) = groceries.blocks.first_mut() else {
//!     panic!("the section opens with a list");
//! };
//! let salt = Line::new(vec![Inline::Text(Text::new("salt".to_owned()))]);
//! let paragraph = Block::Paragraph(Paragraph::new(vec![salt]));
//! let body = MarkdownBlock::new(vec![Section::new(None, vec![paragraph], Vec::new())]);
//! list.items.push(Item::new(body));
//!
//! let options = WriteOptions::default();
//! let text = write(&parsed.document, &options)?;
//! assert_eq!(text, "# Groceries\n\n- flour\n- yeast\n- salt\n");
//! assert_eq!(write(&parse(&text).document, &options)?, text);
//! # Ok::<(), arky::writer::WriteError>(())
//! ```
//!
//! ## Find every link in a note
//!
//! A static-site tool checks every link of a note. The tree nests: a section holds its blocks,
//! then its subsections; a list item and a table cell each hold a whole markdown block; a
//! paragraph holds lines of inline nodes, and an emphasis holds inline nodes too. A walk that
//! recurses into each reaches every link. The enums are `#[non_exhaustive]`, so each `match`
//! ends with a wildcard arm. A block quote holds raw text: parse its `value` to reach its links.
//!
//! ```rust
//! use arky::parser::ast::{Block, Inline, MarkdownBlock, RawDocument, Section};
//! use arky::parser::parse;
//!
//! fn links(block: &MarkdownBlock, out: &mut Vec<String>) {
//!     for section in &block.sections {
//!         section_links(section, out);
//!     }
//! }
//!
//! fn section_links(section: &Section, out: &mut Vec<String>) {
//!     if let Some(heading) = &section.heading {
//!         inline_links(&heading.children, out);
//!     }
//!     for block in &section.blocks {
//!         match block {
//!             Block::Paragraph(paragraph) => {
//!                 for line in &paragraph.lines {
//!                     inline_links(&line.children, out);
//!                 }
//!             }
//!             Block::List(list) => {
//!                 for item in &list.items {
//!                     links(&item.body, out);
//!                 }
//!             }
//!             Block::Table(table) => {
//!                 for row in table.head.iter().chain(&table.rows) {
//!                     for cell in &row.cells {
//!                         links(&cell.body, out);
//!                     }
//!                 }
//!             }
//!             _ => {}
//!         }
//!     }
//!     for subsection in &section.subsections {
//!         section_links(subsection, out);
//!     }
//! }
//!
//! fn inline_links(inlines: &[Inline], out: &mut Vec<String>) {
//!     for inline in inlines {
//!         match inline {
//!             Inline::Link(link) => out.push(link.target.clone()),
//!             Inline::Emphasis(emphasis) => inline_links(&emphasis.children, out),
//!             Inline::Strong(strong) => inline_links(&strong.children, out),
//!             _ => {}
//!         }
//!     }
//! }
//!
//! let parsed = parse("# Plan\n\nSee [spec](spec.md).\n\n- [board](https://example.com/board)\n");
//! let RawDocument::Valid(document) = &parsed.document else {
//!     panic!("a file with no frontmatter is valid");
//! };
//! let mut out = Vec::new();
//! links(&document.body, &mut out);
//! assert_eq!(out, ["spec.md", "https://example.com/board"]);
//! ```
//!
//! ## Keep documents as JSON
//!
//! A service stores parsed documents in a JSON column or sends them over an API.
//! [`to_json`](parser::to_json) writes the whole tree with short field names (`bd` for the body,
//! `se` for sections, `bl` for blocks, `hd` for a heading, `data` for the frontmatter) and a `t`
//! tag on each variant of the node enums, such as `"t":"paragraph"`, at any depth. Frontmatter
//! values are plain JSON with no tag, except a date or a date-time, written as
//! `{"$date":...}`, and a time of day, written as `{"t":"time","v":"07:32:00"}`.
//! [`from_json`](parser::from_json) reads it back.
//!
//! ```rust
//! use arky::parser::ast::RawDocument;
//! use arky::parser::{from_json, parse, to_json};
//!
//! let document = parse("---\ntitle: Notes\n---\n# Plan\nDraft the outline.\n").document;
//! let json = to_json(&document)?;
//! assert_eq!(
//!     json,
//!     concat!(
//!         r#"{"t":"valid","bd":{"se":[{"bl":[{"t":"paragraph","li":[{"ch":[{"t":"text","#,
//!         r#""v":"Draft the outline."}]}]}],"hd":{"ch":[{"t":"text","v":"Plan"}]},"ss":[]}]},"#,
//!         r#""data":{"title":"Notes"}}"#,
//!     ),
//! );
//! let back: RawDocument = from_json(&json)?;
//! assert_eq!(back, document);
//! # Ok::<(), serde_json::Error>(())
//! ```
//!
//! ## Ship a note with its pictures
//!
//! A tool exports a recipe and the pictures it embeds as one self-contained file, then unpacks it
//! somewhere else. [`BundledDocument::from_file`](bundle::BundledDocument::from_file) reads the
//! note and every file its image embeds name, through an os-shim `System`: the real disk, or the
//! in-memory `MemorySystem` used here. [`forms::encode`] writes the bundle as one markdown file,
//! each picture inside its embed as a data URI that keeps the picture's path. The zip form,
//! [`Form::Zip`](forms::Form::Zip), holds `index.md` and each file at its path instead.
//!
//! ```rust
//! use std::path::Path;
//!
//! use arky::bundle::{BundledDocument, Missing};
//! use arky::forms::{Form, decode, encode};
//! use arky::writer::WriteOptions;
//! use os_shim::System as _;
//! use os_shim::mock::MemorySystem;
//!
//! # tokio::runtime::Builder::new_current_thread().build()?.block_on(async {
//! let notes = MemorySystem::new()
//!     .with_file("/recipes/bread.md", b"# Bread\n\n![Loaf](images/loaf.gif)\n")?
//!     .with_file("/recipes/images/loaf.gif", b"GIF89a")?;
//! let path = Path::new("/recipes/bread.md");
//! let bundle = BundledDocument::from_file(&notes, path, Missing::Refuse).await?;
//! assert_eq!(bundle.files(), ["images/loaf.gif"]);
//!
//! let options = WriteOptions::default();
//! let one_file = encode(&bundle, Form::Markdown, &options)?;
//! assert_eq!(
//!     String::from_utf8(one_file.clone())?,
//!     "# Bread\n\n![Loaf](data:image/gif;name=images/loaf.gif;base64,R0lGODlh)\n",
//! );
//!
//! let unpacked = decode(&one_file, Form::Markdown, Missing::Refuse)?;
//! assert_eq!(unpacked, bundle);
//! let elsewhere = MemorySystem::new();
//! unpacked.to_file(&elsewhere, Path::new("/copy/bread.md"), &options).await?;
//! assert_eq!(
//!     elsewhere.read_to_string(Path::new("/copy/bread.md")).await?,
//!     "# Bread\n\n![Loaf](images/loaf.gif)\n",
//! );
//! assert_eq!(
//!     elsewhere.read_to_string(Path::new("/copy/images/loaf.gif")).await?,
//!     "GIF89a",
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! # })?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Pass a document between processes in the binary form
//!
//! Two steps of a pipeline hand a document and its files to each other with no text to parse.
//! With the `binary-format` feature, [`forms::encode`] with `Form::Binary` writes a 6-byte header
//! (`arky`, a zero byte and the version byte, 4) and then an rkyv archive of the tree and the raw
//! bytes of each file. Bytes with another header fail to decode. [`forms::stored`] gives the text
//! and the files a store keeps for binary input.
//!
#![cfg_attr(
    feature = "binary-format",
    doc = r#"
```rust
use arky::bundle::Missing;
use arky::forms::{self, DecodeError, Form};
use arky::writer::WriteOptions;

let options = WriteOptions::default();
let bundle = forms::decode(b"* bread\n", Form::Markdown, Missing::Refuse)?;
let bytes = forms::encode(&bundle, Form::Binary, &options)?;
assert_eq!(&bytes[..6], b"arky\0\x04");
assert_eq!(forms::decode(&bytes, Form::Binary, Missing::Refuse)?, bundle);

let stored = forms::stored(&bytes, Form::Binary, &options, Missing::Refuse)?;
assert_eq!(stored.text, "- bread\n");
assert!(matches!(
    forms::decode(b"not arky", Form::Binary, Missing::Refuse),
    Err(DecodeError::NotArky)
));
# Ok::<(), Box<dyn std::error::Error>>(())
```
"#
)]
//!
//! ## Describe the JSON to a `TypeScript` client
//!
//! A web editor receives documents as JSON and checks them before it renders them. The node and
//! value types have `ts_definition` (the `TypeScript` type), `zod_schema` (a Zod 4 schema, as
//! `TypeScript` text) and `json_schema` (a JSON Schema document), generated by tixschema from the
//! same types serde writes, so the short field names and the `t` tags match the JSON. Each enum
//! whose variants hold no data also has `enum_members`, its variants' names. A build step writes
//! the definitions of the types it needs into one `.ts` file. Call a generic type with its
//! default filled in, as `<Document>::ts_definition()`.
//!
//! ```rust
//! use arky::parser::ast::{Document, FrontmatterFormat, RawDocument};
//! use serde_json::json;
//!
//! assert_eq!(
//!     FrontmatterFormat::json_schema(),
//!     json!({ "type": "string", "enum": ["toml", "yaml"] }),
//! );
//! assert_eq!(FrontmatterFormat::enum_members(), ["toml", "yaml"]);
//! assert!(<Document>::ts_definition().contains("bd: MarkdownBlock<E>;"));
//! assert!(<RawDocument>::zod_schema().contains(r#"t: z.literal("valid"),"#));
//! ```
//!
//! The `TypeScript` and Zod text carries each type's documentation as comments and
//! descriptions, so it changes when the documentation does. `json_schema` and `enum_members`
//! carry none.
//!
//! ## From the command line
//!
//! The `arky-cli` crate builds a command, `arky`, from these calls and from the stores of the
//! `arky-store` crate. A path with no store is a file on disk.
//!
//! ```console
//! $ cargo install arky-cli
//! $ printf 'Plan\n====\n\n* draft\n* review\n' > plan.md
//! $ arky get plan.md
//! # Plan
//!
//! - draft
//! - review
//! $ arky get plan.md -f json > plan.json
//! $ arky write plan.md -f json -i plan.json
//! $ arky get report.md -f zip -o report.zip
//! $ arky write copy/report.md -f zip -i report.zip
//! $ arky get report.md -o out/report.md --embed
//! $ arky get report.md -f binary | arky write archive/report.md -f binary
//! $ arky get post.md --frontmatter toml
//! $ arky fix notes
//! ```
//!
//! | Command or option | Library call |
//! | --- | --- |
//! | `arky get PATH` | `arky_store::document::get`, which gives a [`BundledDocument`](bundle::BundledDocument), then [`forms::encode`] in the form `-f` names, markdown by default |
//! | `arky get PATH -o FILE` | `arky_store::document::get`, then in markdown `arky_store::document::put_bundle` at `FILE`, which writes each embedded file beside it; in another form, or in markdown with `--embed`, which puts each file inside as a data URI, [`forms::encode`] written to `FILE` |
//! | `arky write PATH` | `arky_store::document::put`, which stores the text and files [`forms::stored`] gives for standard input or `-i FILE`; markdown from `-i FILE` is read with `arky_store::document::get`, which brings the files it embeds from beside `FILE`, and stored with `arky_store::document::put_bundle` |
//! | `-f` with `markdown`, `json`, `zip` or `binary` | [`forms::Form`]; `binary` needs the `binary-format` feature, which `arky-cli` turns on by default |
//! | `--frontmatter` with `yaml` or `toml` | [`WriteOptions::frontmatter`](writer::WriteOptions::frontmatter) |
//! | `--allow-missing` | [`Missing::Keep`](bundle::Missing::Keep) in place of [`Missing::Refuse`](bundle::Missing::Refuse) |
//! | `-s STORE`, `ARKY_STORE` or `default` in `arky.toml`, and `PATH` written as a store string such as `arky://<name>/<path>` | `Backends::open` and `Backends::open_path` of `arky-store`, which open the folder, `MinIO`, AWS S3, `MongoDB` or saved store a store string names, in place of the disk |
//! | `arky fix [PATH]` | `FileStore::remove_expired_locks` of `arky-store`, or `remove_expired_locks` of the store `-s` names, which removes the expired locks stopped saves left behind |
//! | `arky asset finish-deletions -s STORE` | `finish_deletions` of the store `-s` names, a `MongoDB` store |
//!
//! # Features
//!
//! `zod` and `jsonschema` are on by default.
//!
//! - `zod` and `jsonschema` turn on no code. In every build, tixschema generates `ts_definition`,
//!   `zod_schema` and `json_schema` on the node and value types, and `enum_members` on those
//!   enums whose variants hold no data. Add `serde_json` to name the `serde_json::Value` that
//!   `json_schema` returns.
//! - `mongodb` adds tixschema's `MongoDB` layer to the node and value types: a `MONGO_FIELDS` const
//!   of typed paths that build `bson` filters and updates, typed operations such as `find_one`
//!   and `insert_one` over a `mongodb::Collection<bson::Document>`, and `from_bson_with`, which
//!   reads a stored row and lets a callback repair what does not read. It adds the `mongodb`
//!   driver and `bson`. Add `bson = { version = "3.1", features = ["serde"] }` to build filters
//!   and rows, and the `mongodb` driver, version 3.9 with its `bson-3` feature, plus an async
//!   runtime to run the operations.
//! - `binary-format` adds the binary form: `Form::Binary`, `forms::BINARY_MAGIC`,
//!   `forms::BINARY_VERSION`, `writer::write_archived`, the archived types in [`parser::ast`]
//!   and the `parser::archive` module. It adds rkyv. Add `rkyv = "0.8"` only to name
//!   `rkyv::rancor::Error`, which `DecodeError::Binary` and `EncodeError::Binary` hold.
//!
//! ```toml
//! [dependencies]
//! arky = { version = "0.2", features = ["binary-format", "mongodb"] }
//! bson = { version = "3.1", features = ["serde"] }
//! ```
//!
//! With `mongodb`, a typed path builds a filter, and the tree reads back from a BSON row:
//!
#![cfg_attr(
    feature = "mongodb",
    doc = r##"
```rust
use arky::parser::ast::RawDocument;
use arky::parser::parse;

let valid = <RawDocument>::MONGO_FIELDS.is_valid().into_document();
assert_eq!(valid.to_string(), r#"{ "t": { "$eq": "valid" } }"#);

let document = parse("---\nstatus: open\n---\n# Task\n").document;
let row = bson::serialize_to_document(&document)?;
let back: RawDocument = bson::deserialize_from_document(row)?;
assert_eq!(back, document);
# Ok::<(), Box<dyn std::error::Error>>(())
```
"##
)]
//!
//! # Limits
//!
//! - A write gives arky's form, not the bytes that were read: `#` headings, `-` bullets, `.`
//!   after list numbers, `*em*` and `**strong**`, `***` breaks, backtick fences, inline links in
//!   place of reference links, padded pipe tables (grid tables when a cell takes more than one
//!   line or the header is not one row), `\n` line endings and one blank line between blocks.
//!   The writer departs from these forms only where one would read back as something else: a
//!   list right after a list of its kind takes `*` or `)`, a level 1 or 2 heading that holds a
//!   line break is underlined with `===` or `---`, a section deeper than level 6 goes in a
//!   `markdown` code block, a fence whose info string holds a backtick is made of `~`, and a
//!   `***` line inside a `*` item is written `___`. From the second write on, the text does not
//!   change.
//! - A valid document's frontmatter is written as YAML unless
//!   [`WriteOptions::frontmatter`](writer::WriteOptions::frontmatter) is
//!   [`FrontmatterFormat::Toml`](parser::ast::FrontmatterFormat::Toml), whatever syntax the file
//!   used. Its comments are not kept, YAML anchors are written out as copies, and a key written
//!   twice becomes one key holding a list. A frontmatter no entry of reads is written back as it
//!   was read, in its own syntax, comments included.
//! - A block quote holds raw text. Its [`value`](parser::ast::BlockQuote::value) is its lines
//!   without their `>` marks, and nothing inside it is parsed.
//! - A heading's level is not stored. It is the depth of its section in its
//!   [`MarkdownBlock`](parser::ast::MarkdownBlock), and each list item, table cell and `markdown`
//!   code block counts levels from one again.
//! - JSON and the binary form do not carry the `kind` field where an extension keeps its node.
//!   A `markdown` code block that [`parse`](parser::parse) read as a nested document comes back
//!   as a plain code block; parse the written text again to get the nested document back.
//! - JSON writes each date and date-time as an instant in UTC, to the millisecond, and each time
//!   of day to the millisecond. An offset is lost, a date-time with no offset reads back as one at
//!   `+00:00`, and a date-time at midnight UTC reads back as a date.
//! - A bundle follows image embeds only, `![alt](path)` and `![[name]]`. It does not follow
//!   links, or embeds inside code blocks.
//! - Nesting is capped. List items, markdown blocks and table cells nest at most 40 deep together,
//!   inline nodes at most 1000 deep, and frontmatter values at most 400 deep. Past a cap the
//!   marks stay text, or the frontmatter entry is dropped.
//! - Most types are `#[non_exhaustive]`. Build nodes with their `new` functions, or `default()`
//!   for `ThematicBreak` and `LineBreak`; start options from `default()` and set their fields;
//!   and give each match on `Block`, `Inline`, `Value` or `RawDocument` a wildcard arm.
//!
//! # Where to go next
//!
//! - [`parser::parse`] and [`parser::ast`] for the tree, and [`parser::ast::Value`] for the
//!   frontmatter's values.
//! - [`parser::extension::Extension`] for node types of your own, with source spans.
//! - [`writer::write`] for the written form.
//! - [`bundle::BundledDocument`] and [`forms`] for a document with its files.
//! - [`GRAMMAR.md`](https://github.com/tixena/arky/blob/master/crates/arky/GRAMMAR.md), the
//!   grammar the parser follows and the form each construct is written in.

extern crate alloc;

tixschema::decode_with_foundation!();
tixschema::mongodb_foundation!();

pub mod bundle;
pub mod forms;
pub mod parser;
pub mod value;
pub mod writer;
