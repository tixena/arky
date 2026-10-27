//! The errors a document fails to parse or to be rebuilt with.

use core::convert::Infallible;
use core::ops::Range;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tixschema::model_schema;

use crate::writer::WriteError;

/// Why a document could not be parsed, a frontmatter entry was dropped or a
/// frontmatter is broken, and where.
#[model_schema()]
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{message} (at byte {})", span.start)]
#[non_exhaustive]
pub struct ParseError {
    pub message: String,
    pub span: Range<usize>,
}

impl ParseError {
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
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Write(#[from] WriteError),
}
