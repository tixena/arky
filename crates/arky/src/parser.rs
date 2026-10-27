//! Markdown parsing: a source file into its document tree. The grammar it
//! follows is written in `GRAMMAR.md` at the root of this crate.

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
    /// The construct stays the plain node arky built.
    #[default]
    Fallback,
}

/// How a file is parsed.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ParseOptions {
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
    pub document: RawDocument<E>,
    pub dropped: Vec<ParseError>,
}

impl ParseOptions {
    /// A parse that an extension's error fails, and the first entry the
    /// frontmatter drops or a frontmatter of which no entry reads.
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            on_extension_error: OnExtensionError::Fail,
        }
    }
}

/// Parses a markdown file into its document tree, with arky's markdown
/// extension alone.
///
/// A frontmatter entry that does not read is dropped and listed, and a file
/// whose frontmatter no entry of reads is an `InvalidDocument`, its
/// frontmatter's error listed.
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let Ok(parsed) = parse_with_set::<Markdown, Infallible>(source, &ParseOptions::default());
    parsed
}

/// Parses a markdown file into its document tree, with arky's markdown
/// extension first and `E` after it.
///
/// # Errors
///
/// Returns an error only in a strict parse: when the frontmatter drops an
/// entry, when no entry of it reads, or when an extension claims a construct
/// and cannot read it.
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
/// The plain tree is written in arky's form, its frontmatter in YAML, and
/// parsed again with `E`.
///
/// # Errors
///
/// Returns `RebuildError::Write`, naming the key, when the `Toml` args of a
/// code block hold a null, which TOML cannot hold, or naming the line when
/// the text of an invalid frontmatter holds one that would close it, and
/// `RebuildError::Parse` when the parse fails, as `parse_with` does.
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

/// Reads a document from the JSON `to_json` writes, at any depth arky reads.
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

/// Writes `document` as JSON, at any depth arky reads. No node writes its
/// `kind`; `rebuild_with` builds it again.
///
/// # Errors
///
/// Returns `serde_json`'s error when a value cannot be written as JSON.
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
pub fn read_toml(source: &str, range: Range<usize>) -> Result<Mapping, ParseError> {
    toml::parse(source, range).map(located::Mapping::into_model)
}

/// Reads the TOML written on `lines`, ranges of `source`, as consecutive
/// lines of one text, the way `read_toml` reads a range.
///
/// # Errors
///
/// Returns the error of the first entry the text drops, as a strict parse
/// does, or of the text when no entry of it reads. Its span counts from the
/// start of `source`.
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
pub fn read_yaml(source: &str, range: Range<usize>) -> Result<Mapping, ParseError> {
    yaml::parse(source, range).map(located::Mapping::into_model)
}

/// Reads the YAML written on `lines`, ranges of `source`, as consecutive
/// lines of one text, the way `read_yaml` reads a range.
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
