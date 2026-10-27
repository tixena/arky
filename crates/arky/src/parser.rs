//! Reads markdown text into the document tree of [`ast`], and moves that tree to and from JSON.
//!
//! [`parse`] reads a file with arky's [`Markdown`] extension, and [`parse_with`] adds an
//! [`Extension`] of your own after it. Both give a [`Parsed`]: the [`RawDocument`] and each
//! frontmatter entry the parse could not read. [`to_json`] and [`from_json`] write the tree as
//! JSON and read it back, and [`rebuild_with`] builds the extension nodes again on a tree read
//! back. [`read_toml`], [`read_yaml`], [`read_toml_lines`] and [`read_yaml_lines`] read TOML or
//! YAML from part of a text, the way a frontmatter is read.
//!
//! The submodules hold the tree ([`ast`]), the tree with a byte span on every node, as an
//! extension receives it ([`located`]), the extension trait ([`extension`]) and the errors
//! ([`error`]). With the `binary-format` feature, `archive` holds the rkyv adapters of the
//! chrono types a frontmatter value holds.
//!
//! The parser follows
//! [`GRAMMAR.md`](https://github.com/tixena/arky/blob/master/crates/arky/GRAMMAR.md): what each
//! construct reads as, how a frontmatter is read, and where arky differs from `CommonMark`.
//!
//! A frontmatter is TOML between `+++` lines or YAML between `---` lines, at the top of the
//! file. A valid document's tree does not record which syntax the file used:
//!
//! ```rust
//! use arky::parser::parse;
//!
//! let toml = parse("+++\ntitle = \"Bread\"\nservings = 2\n+++\n# Bread\n").document;
//! let yaml = parse("---\ntitle: Bread\nservings: 2\n---\n# Bread\n").document;
//! assert_eq!(toml, yaml);
//! ```
//!
//! When no entry of the frontmatter reads, the document is [`RawDocument::Invalid`], whose
//! [`InvalidFrontmatter`](ast::InvalidFrontmatter) keeps the syntax and the text, and a write
//! gives that text back as it was read.

#[cfg(feature = "binary-format")]
pub mod archive;
pub mod ast;
pub mod error;
pub mod extension;
pub mod located;

mod block;
mod claim;
mod cursor;
pub(crate) mod definition;
pub(crate) mod entity;
pub(crate) mod frontmatter;
pub(crate) mod html;
pub(crate) mod inline;
pub(crate) mod lexer;
pub(crate) mod line;
#[cfg(feature = "binary-format")]
pub(crate) mod mirror;
pub(crate) mod scalar;
pub(crate) mod table;
mod toml;
mod yaml;

use core::convert::Infallible;
use core::ops::Range;

use serde::ser::Error as _;
use serde::{Deserialize as _, Serialize as _};

use crate::parser::ast::{Document, InvalidDocument, Kind, Mapping, RawDocument};
use crate::parser::error::{Failure, ParseError, RebuildError};
use crate::parser::extension::{Context, Extension, Frame, Markdown, With};
use crate::writer::{self, WriteOptions};

/// What happens when an extension claims a construct and cannot read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OnExtensionError {
    /// The extension's error fails the parse, and so do the first entry the
    /// frontmatter drops and a frontmatter of which no entry reads.
    Fail,
    /// The construct stays the plain node arky built, and the parse goes on.
    #[default]
    Fallback,
}

/// How a file is parsed.
///
/// The struct is `#[non_exhaustive]`: start from `ParseOptions::default()` or
/// [`ParseOptions::strict`], then set its field.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ParseOptions {
    /// What an extension's error, a dropped frontmatter entry and an invalid frontmatter do to
    /// the parse. [`OnExtensionError::Fallback`] by default.
    pub on_extension_error: OnExtensionError,
}

/// What a parse gives: the document, and each frontmatter entry it could not
/// read, with its span into the parsed text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Parsed<E = Markdown>
where
    E: Extension,
{
    /// The document tree.
    pub document: RawDocument<E>,
    /// Each frontmatter entry the parse left out, in file order: why it does not read, and the
    /// byte range of the whole entry in the source. A write leaves the dropped text out.
    ///
    /// When no entry of the frontmatter reads, it holds the frontmatter's one error, and the
    /// document is [`RawDocument::Invalid`]: a write gives the whole frontmatter back as it was
    /// read, the entry that does not read included.
    pub dropped: Vec<ParseError>,
}

impl ParseOptions {
    /// The options of a strict parse, [`OnExtensionError::Fail`]: an extension's error, the
    /// first entry the frontmatter drops and a frontmatter of which no entry reads each fail it.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            on_extension_error: OnExtensionError::Fail,
        }
    }
}

/// Parses a markdown file into its document tree, with arky's [`Markdown`] extension alone.
///
/// The parse never fails: markdown that no rule recognizes is text. A frontmatter entry that
/// does not read is left out and listed in [`Parsed::dropped`]. When no entry of the frontmatter
/// reads, the document is [`RawDocument::Invalid`], which keeps the frontmatter's text, and the
/// frontmatter's error is listed.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::{RawDocument, Value};
/// use arky::parser::parse;
/// use arky::writer::{WriteOptions, write};
///
/// let options = WriteOptions::default();
/// let source = "---\nstatus: open\nbad: [\n---\n# Task\n";
/// let parsed = parse(source);
/// let RawDocument::Valid(document) = &parsed.document else {
///     panic!("one entry of the frontmatter reads");
/// };
/// let frontmatter = document.frontmatter.as_ref().expect("the file has a frontmatter");
/// assert_eq!(frontmatter.get("status"), Some(&Value::String("open".to_owned())));
/// assert_eq!(parsed.dropped.len(), 1);
/// assert_eq!(&source[parsed.dropped[0].span.clone()], "bad: [");
/// assert_eq!(write(&parsed.document, &options)?, "---\nstatus: open\n---\n# Task\n");
///
/// let invalid = parse("+++\ndue = tomorrow\n+++\n");
/// assert!(matches!(invalid.document, RawDocument::Invalid(_)));
/// assert_eq!(invalid.dropped[0].to_string(), "`tomorrow` is not a value (at byte 10)");
/// assert_eq!(write(&invalid.document, &options)?, "+++\ndue = tomorrow\n+++\n");
/// # Ok::<(), arky::writer::WriteError>(())
/// ```
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let Ok(parsed) = parse_with_set::<Markdown, Infallible>(source, &ParseOptions::default());
    parsed
}

/// Parses a markdown file into its document tree, with arky's [`Markdown`] extension first and
/// `E` after it.
///
/// The extensions are the pair `(Markdown, E)`, which [`With`] names, so a node `E` claims sits
/// in its `kind` as [`Either::Second`](extension::Either::Second). When no entry of the
/// frontmatter reads, only [`Markdown`] reads the body, and `E` claims nothing in it.
///
/// # Errors
///
/// Returns an error only in a strict parse: when the frontmatter drops an
/// entry, when no entry of it reads, or when an extension claims a construct
/// and cannot read it.
///
/// # Examples
///
/// ```rust
/// use arky::parser::extension::Plain;
/// use arky::parser::{ParseOptions, parse_with};
///
/// let source = "---\nstatus: open\nbad: [\n---\n# Task\n";
/// let lenient = parse_with::<Plain>(source, &ParseOptions::default())?;
/// assert_eq!(lenient.dropped.len(), 1);
///
/// let strict = parse_with::<Plain>(source, &ParseOptions::strict());
/// let error = strict.expect_err("a strict parse fails on a dropped entry");
/// assert_eq!(error.message, lenient.dropped[0].message);
/// # Ok::<(), arky::parser::error::ParseError>(())
/// ```
pub fn parse_with<E>(source: &str, options: &ParseOptions) -> Result<Parsed<With<E>>, ParseError>
where
    E: Extension,
{
    parse_with_set::<With<E>, ParseError>(source, options)
}

/// The located tree `source` parses into, for arky's own tests of spans.
#[cfg(test)]
pub(crate) fn parse_located<T>(source: &str, options: &ParseOptions) -> located::Document<T>
where
    T: Extension,
{
    let strict = options.on_extension_error == OnExtensionError::Fail;
    let Ok((frontmatter, body_start)) =
        frontmatter::parse::<Infallible>(source, text_start(source), strict);
    let Ok(document) = claimed::<T, Infallible>(source, frontmatter, body_start, strict);
    document
}

/// Builds the extension nodes of `document` again, for a document read from
/// storage that keeps only the plain tree, such as the JSON `to_json` writes
/// and `from_json` reads.
///
/// The plain tree is written in arky's form, a valid frontmatter in YAML and
/// one no entry of reads as it was read, and parsed again with `E`.
///
/// # Errors
///
/// Returns [`RebuildError::Write`], naming the key, when the `Toml` args of a
/// code block hold a null, which TOML cannot hold, or naming the line when
/// the text of an invalid frontmatter holds one that would close it, and
/// [`RebuildError::Parse`] when the parse fails, as [`parse_with`] does.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::RawDocument;
/// use arky::parser::extension::{Plain, With};
/// use arky::parser::{ParseOptions, from_json, parse_with, rebuild_with, to_json};
///
/// let options = ParseOptions::default();
/// let parsed = parse_with::<Plain>("```markdown\n# Inner\n```\n", &options)?.document;
/// let back: RawDocument<With<Plain>> = from_json(&to_json(&parsed)?)?;
/// assert_ne!(back, parsed);
/// assert_eq!(rebuild_with::<Plain>(&back, &options)?, parsed);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn rebuild_with<E>(
    document: &RawDocument<With<E>>,
    options: &ParseOptions,
) -> Result<RawDocument<With<E>>, RebuildError>
where
    E: Extension,
{
    let text = writer::write(document, &WriteOptions::default())?;
    Ok(parse_with::<E>(&text, options)?.document)
}

/// Reads a document from the JSON [`to_json`] writes, however deep its tree.
///
/// JSON holds no extension node, so every `kind` comes back empty: a `markdown` code block that
/// [`parse`] read as a nested document comes back as a plain code block, and
/// [`rebuild_with`] builds such nodes again. A frontmatter value that is a mapping whose every
/// key starts with `$` does not read, since the JSON writes a date as one.
///
/// # Errors
///
/// Returns `serde_json`'s error when the text is not such a document.
pub fn from_json<E>(text: &str) -> Result<RawDocument<E>, serde_json::Error>
where
    E: Extension,
{
    let mut reader = serde_json::Deserializer::from_str(text);
    reader.disable_recursion_limit();
    let document = RawDocument::deserialize(serde_stacker::Deserializer::new(&mut reader))?;
    reader.end()?;
    Ok(document)
}

/// Writes `document` as JSON, however deep its tree.
///
/// Each field has a short name (`bd` for a body, `data` for the frontmatter, `se` for sections,
/// `bl` for blocks), each variant of a node has a `t` tag, and no node writes its `kind` or a
/// span. A date or a date-time is written as `{"$date":{"$numberLong":"<milliseconds>"}}` in
/// UTC, and a time of day as `{"t":"time","v":"07:32:00"}`. The JSON keeps a time to the
/// millisecond and no offset: a date-time reads back as one at `+00:00`, or as a date when it is
/// at midnight UTC.
///
/// # Errors
///
/// Returns `serde_json`'s error when a value cannot be written as JSON.
///
/// # Examples
///
/// ```rust
/// use arky::parser::{parse, to_json};
///
/// let document = parse("+++\ndue = 2026-10-20\nstarts = 07:32:00\n+++\n").document;
/// assert_eq!(
///     to_json(&document)?,
///     concat!(
///         r#"{"t":"valid","bd":{"se":[]},"data":{"#,
///         r#""due":{"$date":{"$numberLong":"1792454400000"}},"#,
///         r#""starts":{"t":"time","v":"07:32:00"}}}"#,
///     ),
/// );
/// # Ok::<(), serde_json::Error>(())
/// ```
pub fn to_json<E>(document: &RawDocument<E>) -> Result<String, serde_json::Error>
where
    E: Extension,
{
    let mut text = Vec::new();
    document.serialize(serde_stacker::Serializer::new(
        &mut serde_json::Serializer::new(&mut text),
    ))?;
    String::from_utf8(text).map_err(serde_json::Error::custom)
}

/// Where the text of `source` starts: past a byte-order mark, if one opens
/// it.
fn text_start(source: &str) -> usize {
    if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }
}

/// The located tree of `source`, whose frontmatter is read and whose body
/// starts at `body_start`, every node offered to the extensions `T`.
fn claimed<T, F>(
    source: &str,
    frontmatter: Option<Result<located::Frontmatter, located::BrokenFrontmatter>>,
    body_start: usize,
    strict: bool,
) -> Result<located::Document<T>, F>
where
    T: Extension,
    F: Failure,
{
    let (body, _, cells) = block::parse(source, body_start);
    let mut document = located::Document {
        body,
        frontmatter,
        kind: Kind(None),
        span: 0..source.len(),
    };
    let context = Context {
        column: 0,
        depth: 0,
        frame: Frame::NONE,
        lines: None,
        source,
        strict,
    };
    claim::document(&mut document, &context, &cells).or_else(F::fail)?;
    Ok(document)
}

/// Builds the located tree, offers every node to the extensions `T`, then
/// moves it into the model. A body whose frontmatter no entry of reads is
/// offered to arky's markdown extension alone.
fn parse_with_set<T, F>(source: &str, options: &ParseOptions) -> Result<Parsed<T>, F>
where
    T: Extension,
    F: Failure,
{
    let strict = options.on_extension_error == OnExtensionError::Fail;
    let (frontmatter, body_start) = frontmatter::parse(source, text_start(source), strict)?;
    if let Some(Err(broken)) = frontmatter {
        let dropped = vec![broken.error.clone()];
        let read = claimed::<Markdown, F>(source, None, body_start, strict)?;
        let document = RawDocument::Invalid(InvalidDocument {
            body: read.body.into_model(),
            frontmatter: broken.into_model(),
        });
        return Ok(Parsed { document, dropped });
    }
    let read = claimed::<T, F>(source, frontmatter, body_start, strict)?;
    let (mapping, dropped) = read
        .frontmatter
        .and_then(Result::ok)
        .map_or((None, Vec::new()), |kept| {
            (Some(kept.mapping), kept.dropped)
        });
    let document = RawDocument::Valid(Document {
        body: read.body.into_model(),
        frontmatter: mapping,
        kind: read.kind,
    });
    Ok(Parsed { document, dropped })
}

/// Reads the TOML written at `range` of `source` into a mapping, the way
/// TOML frontmatter is read.
///
/// # Errors
///
/// Returns the error of the first entry the text drops, as a strict parse
/// does, or of the text when no entry of it reads. Its span counts from the
/// start of `source`.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky::parser::read_toml;
///
/// let source = "title = \"Bread\"\nservings = 2";
/// let mapping = read_toml(source, 0..source.len())?;
/// assert_eq!(mapping.get("servings"), Some(&Value::Integer(2)));
///
/// let broken = "title = Notes\ndate = 2026-10-17";
/// let error = read_toml(broken, 0..broken.len()).expect_err("`Notes` is not quoted");
/// assert_eq!(error.message, "`Notes` is not a value");
/// assert_eq!(&broken[error.span.clone()], "Notes");
/// # Ok::<(), arky::parser::error::ParseError>(())
/// ```
pub fn read_toml(source: &str, range: Range<usize>) -> Result<Mapping, ParseError> {
    toml::parse(source, range).map(located::Mapping::into_model)
}

/// Reads the TOML written on `lines`, ranges of `source`, as consecutive
/// lines of one text, the way [`read_toml`] reads a range.
///
/// The lines are joined by `\n`. An extension passes the parts of a claimed block's lines that
/// [`Context::lines_of`] gives, so that a header indented in a
/// list item, or written in a grid cell, reads as plain lines.
///
/// # Errors
///
/// Returns the error of the first entry the text drops, as a strict parse
/// does, or of the text when no entry of it reads. Its span counts from the
/// start of `source`.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky::parser::read_toml_lines;
///
/// let source = "> title = \"Bread\"\n> servings = 2\n";
/// let line = |text: &str| {
///     let start = source.find(text).expect("the source holds the line");
///     start..start + text.len()
/// };
/// let mapping = read_toml_lines(source, &[line("title = \"Bread\""), line("servings = 2")])?;
/// assert_eq!(mapping.get("title"), Some(&Value::String("Bread".to_owned())));
/// assert_eq!(mapping.get("servings"), Some(&Value::Integer(2)));
/// # Ok::<(), arky::parser::error::ParseError>(())
/// ```
pub fn read_toml_lines(source: &str, lines: &[Range<usize>]) -> Result<Mapping, ParseError> {
    read_lines(source, lines, toml::parse)
}

/// Reads the YAML written at `range` of `source` into a mapping, the way
/// YAML frontmatter is read.
///
/// # Errors
///
/// Returns the error of the first entry the text drops, as a strict parse
/// does, or of the text when no entry of it reads. Its span counts from the
/// start of `source`.
///
/// # Examples
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky::parser::read_yaml;
///
/// let source = "intro\ntitle: Bread\nservings: 2\nmore";
/// let start = source.find("title").expect("the source holds a title");
/// let end = source.find("\nmore").expect("the source ends with more");
/// let mapping = read_yaml(source, start..end)?;
/// let keys: Vec<&str> = mapping.keys().map(String::as_str).collect();
/// assert_eq!(keys, ["title", "servings"]);
/// assert_eq!(mapping.get("servings"), Some(&Value::Integer(2)));
/// # Ok::<(), arky::parser::error::ParseError>(())
/// ```
pub fn read_yaml(source: &str, range: Range<usize>) -> Result<Mapping, ParseError> {
    yaml::parse(source, range).map(located::Mapping::into_model)
}

/// Reads the YAML written on `lines`, ranges of `source`, as consecutive
/// lines of one text, the way [`read_yaml`] reads a range, as [`read_toml_lines`] reads TOML.
///
/// # Errors
///
/// Returns the error of the first entry the text drops, as a strict parse
/// does, or of the text when no entry of it reads. Its span counts from the
/// start of `source`.
pub fn read_yaml_lines(source: &str, lines: &[Range<usize>]) -> Result<Mapping, ParseError> {
    read_lines(source, lines, yaml::parse)
}

/// Reads `lines`, ranges of `source` joined by `\n`, with `read`, and places
/// the span of an error back in `source`.
fn read_lines(
    source: &str,
    lines: &[Range<usize>],
    read: fn(&str, Range<usize>) -> Result<located::Mapping, ParseError>,
) -> Result<Mapping, ParseError> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut text = String::new();
    for line in lines {
        if !starts.is_empty() {
            text.push('\n');
        }
        starts.push(text.len());
        text.push_str(&source[line.clone()]);
    }
    let place = |span: &Range<usize>| {
        let at = |offset: usize| {
            let index = starts
                .partition_point(|&start| start <= offset)
                .saturating_sub(1);
            lines
                .get(index)
                .zip(starts.get(index))
                .map_or(offset, |(line, &start)| line.start + (offset - start))
        };
        at(span.start)..at(span.end)
    };
    match read(&text, 0..text.len()) {
        Ok(mapping) => Ok(mapping.into_model()),
        Err(mut error) => {
            error.span = place(&error.span);
            Err(error)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
