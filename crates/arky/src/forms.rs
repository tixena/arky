//! The forms a document takes outside arky, markdown, JSON, binary and zip,
//! each a pair of functions from bytes to a bundle and back.

use alloc::collections::BTreeMap;
use core::str::from_utf8;

#[cfg(feature = "binary-format")]
use rkyv::de::Pool;
#[cfg(feature = "binary-format")]
use rkyv::rancor::{self, Strategy};
#[cfg(feature = "binary-format")]
use rkyv::util::AlignedVec;
#[cfg(feature = "binary-format")]
use rkyv::with::{DeserializeWith as _, With};

use crate::bundle::{BundledDocument, BundledDocumentError, Missing};
use crate::parser::ast::RawDocument;
#[cfg(feature = "binary-format")]
use crate::parser::mirror::{ArchivedBundledDocumentMirror, BundledDocumentMirror};
use crate::parser::{from_json, parse, to_json};
#[cfg(feature = "binary-format")]
use crate::writer::write_archived;
use crate::writer::{self, WriteError, WriteOptions};

/// `arky` and a zero byte, the bytes the binary form opens with.
#[cfg(feature = "binary-format")]
pub const BINARY_MAGIC: &[u8; 5] = b"arky\0";

/// The version of the archived types, the byte after `BINARY_MAGIC`.
#[cfg(feature = "binary-format")]
pub const BINARY_VERSION: u8 = 4;

/// A form a document takes outside arky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Form {
    #[cfg(feature = "binary-format")]
    Binary,
    Json,
    Markdown,
    Zip,
}

/// Why bytes do not hold a document in a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    #[cfg(feature = "binary-format")]
    #[error(transparent)]
    Binary(#[from] rancor::Error),
    #[error(transparent)]
    BundledDocument(#[from] BundledDocumentError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[cfg(feature = "binary-format")]
    #[error("not arky's binary form: the bytes do not open with `arky` and a zero byte")]
    NotArky,
    #[error("not UTF-8 at byte {offset}")]
    NotUtf8 { offset: usize },
    #[cfg(feature = "binary-format")]
    #[error("arky's binary form version {found}, and this arky reads version {BINARY_VERSION}")]
    Version { found: u8 },
}

/// Why a document could not be put into a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    #[cfg(feature = "binary-format")]
    #[error(transparent)]
    Binary(#[from] rancor::Error),
    #[error(transparent)]
    BundledDocument(#[from] BundledDocumentError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// Why the text a store keeps could not be made from bytes in a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FormError {
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Encode(#[from] EncodeError),
}

/// The text a store keeps for a document, and the files it keeps beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoredBundle {
    pub files: BTreeMap<String, Vec<u8>>,
    pub text: String,
}

/// The bytes of `bundle` in `form`. `options` names the frontmatter syntax of
/// the markdown and zip forms.
///
/// Markdown and JSON hold each file as a data URI in its embed, and zip holds
/// `index.md` and each file at its path.
///
/// # Errors
///
/// The writer's error for the markdown form, `serde_json`'s or rkyv's for
/// the others, and the bundle's for a tree it cannot embed or a zip it cannot
/// write.
pub fn encode(
    bundle: &BundledDocument,
    form: Form,
    options: &WriteOptions,
) -> Result<Vec<u8>, EncodeError> {
    match form {
        #[cfg(feature = "binary-format")]
        Form::Binary => {
            let archived = rkyv::to_bytes::<rancor::Error>(With::<
                BundledDocument,
                BundledDocumentMirror,
            >::cast(bundle))?;
            Ok([BINARY_MAGIC.as_slice(), &[BINARY_VERSION], &archived].concat())
        }
        Form::Json => {
            let mut text = to_json(&bundle.embedded()?)?;
            text.push('\n');
            Ok(text.into_bytes())
        }
        Form::Markdown => Ok(writer::write(&bundle.embedded()?, options)?.into_bytes()),
        Form::Zip => {
            let mut zip = Vec::new();
            bundle.to_zip(&mut zip, options)?;
            Ok(zip)
        }
    }
}

/// The bundle `bytes` hold in `form`; every data URI becomes a file, and
/// `missing` decides an embed with no file.
///
/// The binary form is deserialized here; a caller that only writes it reads it
/// in place through `stored`.
///
/// # Errors
///
/// `DecodeError::NotUtf8` naming the offset of the first byte that is not, a
/// JSON error, a header that is not arky's or names another version, an
/// rkyv error, and the bundle's error.
pub fn decode(bytes: &[u8], form: Form, missing: Missing) -> Result<BundledDocument, DecodeError> {
    match form {
        #[cfg(feature = "binary-format")]
        Form::Binary => {
            let buffer = aligned(bytes)?;
            let archived = rkyv::access::<ArchivedBundledDocumentMirror, rancor::Error>(&buffer)?;
            let read: BundledDocument = BundledDocumentMirror::deserialize_with(
                archived,
                Strategy::<_, rancor::Error>::wrap(&mut Pool::new()),
            )?;
            let (document, files) = read.into_parts();
            Ok(BundledDocument::new(document, files, missing)?)
        }
        Form::Json => Ok(BundledDocument::new(
            from_json(text(bytes)?)?,
            BTreeMap::new(),
            missing,
        )?),
        Form::Markdown => Ok(BundledDocument::new(
            parse(text(bytes)?).document,
            BTreeMap::new(),
            missing,
        )?),
        Form::Zip => Ok(BundledDocument::from_zip(&mut &*bytes, missing)?),
    }
}

/// The text a store keeps for `bytes` in `form`, arky's form with its
/// frontmatter in the syntax `options` names, and the files beside it.
///
/// `missing` decides an embed with no file. JSON and binary are written, read
/// and written again; binary is read in place with `write_archived`.
///
/// # Errors
///
/// The errors of `decode` and `encode`.
pub fn stored(
    bytes: &[u8],
    form: Form,
    options: &WriteOptions,
    missing: Missing,
) -> Result<StoredBundle, FormError> {
    let bundle = match form {
        #[cfg(feature = "binary-format")]
        Form::Binary => {
            let buffer = aligned(bytes)?;
            let archived = rkyv::access::<ArchivedBundledDocumentMirror, rancor::Error>(&buffer)
                .map_err(DecodeError::from)?;
            let files = rkyv::deserialize::<_, rancor::Error>(&archived.files)
                .map_err(DecodeError::from)?;
            reread(write_archived(&archived.document, options), files, missing)?
        }
        Form::Json => {
            let document: RawDocument = from_json(text(bytes)?).map_err(DecodeError::from)?;
            reread(writer::write(&document, options), BTreeMap::new(), missing)?
        }
        Form::Markdown | Form::Zip => decode(bytes, form, missing)?,
    };
    let (document, files) = bundle.into_parts();
    let text = writer::write(&document, options).map_err(EncodeError::from)?;
    Ok(StoredBundle { files, text })
}

/// `bytes` as text, or the offset of the first byte that is not UTF-8.
fn text(bytes: &[u8]) -> Result<&str, DecodeError> {
    from_utf8(bytes).map_err(|error| DecodeError::NotUtf8 {
        offset: error.valid_up_to(),
    })
}

/// The bytes after arky's binary header, copied where rkyv reads them in place.
#[cfg(feature = "binary-format")]
fn aligned(bytes: &[u8]) -> Result<AlignedVec, DecodeError> {
    let (&version, archived) = bytes
        .strip_prefix(BINARY_MAGIC.as_slice())
        .and_then(<[u8]>::split_first)
        .ok_or(DecodeError::NotArky)?;
    if version != BINARY_VERSION {
        return Err(DecodeError::Version { found: version });
    }
    let mut buffer = AlignedVec::with_capacity(archived.len());
    buffer.extend_from_slice(archived);
    Ok(buffer)
}

/// `written`, a tree written in arky's form, read back as a bundle with `files`.
fn reread(
    written: Result<String, WriteError>,
    files: BTreeMap<String, Vec<u8>>,
    missing: Missing,
) -> Result<BundledDocument, FormError> {
    let text = written.map_err(EncodeError::from)?;
    Ok(BundledDocument::new(parse(&text).document, files, missing).map_err(DecodeError::from)?)
}

#[cfg(test)]
mod tests;
