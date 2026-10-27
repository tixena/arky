//! The errors of a parse and of a rebuild.
//!
//! [`ParseError`] says why a parse failed or a frontmatter entry was dropped, and where.
//! [`RebuildError`] is what [`rebuild_with`](crate::parser::rebuild_with) fails with.
//!
//! `parse_error_schema` is the module tixschema writes beside `ParseError`, as the
//! [`ast`](crate::parser::ast) page describes.

use core::convert::Infallible;
use core::ops::Range;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tixschema::model_schema;

use crate::writer::WriteError;

/// Why a document could not be parsed, a frontmatter entry was dropped or a
/// frontmatter is broken, and where.
///
/// It displays as its message, then `(at byte N)`, where `N` is the start of its span.
#[model_schema()]
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{message} (at byte {})", span.start)]
#[non_exhaustive]
pub struct ParseError {
    /// What does not read, such as `the string is not closed`.
    pub message: String,
    /// The byte range of the source the error points at.
    pub span: Range<usize>,
}

impl ParseError {
    /// An error of `message` that points at `span`, as an extension's `claim` returns one.
    #[must_use]
    pub fn new(message: &str, span: Range<usize>) -> Self {
        Self {
            message: message.to_owned(),
            span,
        }
    }
}

/// What a parse fails with where a strict parse would: `ParseError` returns
/// the error, and `Infallible` goes on past it, for a parse that cannot fail.
pub(crate) trait Failure: Sized {
    fn fail(error: ParseError) -> Result<(), Self>;
}

impl Failure for ParseError {
    fn fail(error: ParseError) -> Result<(), Self> {
        Err(error)
    }
}

impl Failure for Infallible {
    fn fail(_error: ParseError) -> Result<(), Self> {
        Ok(())
    }
}

/// Why a stored tree could not be built again: it could not be written, or
/// the written text could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum RebuildError {
    /// The written text did not parse, which happens only in a strict parse.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// The tree did not write: a code block's `Toml` args hold a null, or an invalid
    /// frontmatter's text holds a line that would close it.
    #[error(transparent)]
    Write(#[from] WriteError),
}
