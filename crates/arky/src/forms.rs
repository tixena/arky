//! The forms a document and its files take outside arky, each a pair of functions from bytes
//! to a bundle and back.
//!
//! [`encode`] puts a [`BundledDocument`] into a [`Form`], [`decode`] reads one back, and
//! [`stored`] gives the text and the files a store keeps for bytes in any form. The forms are:
//!
//! - markdown: one file in arky's form, each embedded file inside its embed as a data URI.
//! - JSON: the tree as [`to_json`] writes it, each embedded file inside its embed as a data URI,
//!   then a newline.
//! - zip: `index.md` and each file at its path.
//! - binary, with the `binary-format` feature: `arky`, a zero byte and the version byte, then an
//!   rkyv archive of the tree and the raw bytes of each file, for passing a document between
//!   processes of one arky version.
//!
//! ```rust
//! use arky::bundle::Missing;
//! use arky::forms::{Form, decode, encode, stored};
//! use arky::writer::WriteOptions;
//!
//! let options = WriteOptions::default();
//! let bundle = decode(b"* bread\n", Form::Markdown, Missing::Refuse)?;
//! let json = encode(&bundle, Form::Json, &options)?;
//! assert!(json.ends_with(b"}\n"));
//! assert_eq!(decode(&json, Form::Json, Missing::Refuse)?, bundle);
//! assert_eq!(stored(&json, Form::Json, &options, Missing::Refuse)?.text, "- bread\n");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

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

/// `arky` and a zero byte, the bytes the binary form opens with. With the `binary-format`
/// feature.
#[cfg(feature = "binary-format")]
pub const BINARY_MAGIC: &[u8; 5] = b"arky\0";

/// The version of the archived types, the byte after [`BINARY_MAGIC`]. With the
/// `binary-format` feature.
///
/// A change to the tree's types changes the archive and raises the version. [`decode`] and
/// [`stored`] refuse bytes of another version with [`DecodeError::Version`].
#[cfg_attr(
    feature = "binary-format",
    doc = r##"
```rust
use arky::bundle::Missing;
use arky::forms::{self, BINARY_MAGIC, BINARY_VERSION, Form};
use arky::writer::WriteOptions;

let bundle = forms::decode(b"# Notes\n", Form::Markdown, Missing::Refuse)?;
let mut bytes = forms::encode(&bundle, Form::Binary, &WriteOptions::default())?;
assert_eq!(&bytes[..5], BINARY_MAGIC);
assert_eq!(bytes[5], BINARY_VERSION);

bytes[5] = 3;
let refused = forms::decode(&bytes, Form::Binary, Missing::Refuse).expect_err("version 3");
assert_eq!(refused.to_string(), "arky's binary form version 3, and this arky reads version 4");
# Ok::<(), Box<dyn std::error::Error>>(())
```
"##
)]
#[cfg(feature = "binary-format")]
pub const BINARY_VERSION: u8 = 4;

/// A form a document takes outside arky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Form {
    /// arky's binary form, with the `binary-format` feature: [`BINARY_MAGIC`],
    /// [`BINARY_VERSION`], then an rkyv archive of the tree and the raw bytes of each file.
    #[cfg(feature = "binary-format")]
    Binary,
    /// The tree as [`to_json`] writes it, each file inside its embed as a data URI, then a
    /// newline.
    Json,
    /// One markdown file in arky's form, each file inside its embed as a data URI.
    Markdown,
    /// A zip of `index.md` and each file at its path.
    Zip,
}

/// Why bytes do not hold a document in a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// rkyv could not check or read the archive after the header. With the `binary-format`
    /// feature.
    #[cfg(feature = "binary-format")]
    #[error(transparent)]
    Binary(#[from] rancor::Error),
    /// The bundle could not be made from the tree: an embed with no file under
    /// [`Missing::Refuse`], or a data URI that does not read.
    #[error(transparent)]
    BundledDocument(#[from] BundledDocumentError),
    /// The text is not the JSON of a document.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The bytes do not open with [`BINARY_MAGIC`]. With the `binary-format` feature.
    #[cfg(feature = "binary-format")]
    #[error("not arky's binary form: the bytes do not open with `arky` and a zero byte")]
    NotArky,
    /// Markdown or JSON bytes are not UTF-8 from byte `offset`.
    #[error("not UTF-8 at byte {offset}")]
    NotUtf8 { offset: usize },
    /// The binary form's version byte is `found`, not [`BINARY_VERSION`]. With the
    /// `binary-format` feature.
    #[cfg(feature = "binary-format")]
    #[error("arky's binary form version {found}, and this arky reads version {BINARY_VERSION}")]
    Version { found: u8 },
}

/// Why a document could not be put into a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// rkyv could not archive the bundle. With the `binary-format` feature.
    #[cfg(feature = "binary-format")]
    #[error(transparent)]
    Binary(#[from] rancor::Error),
    /// The tree with its files embedded did not write, or the zip did not.
    #[error(transparent)]
    BundledDocument(#[from] BundledDocumentError),
    /// `serde_json` could not write the tree.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The markdown did not write, such as a null under TOML frontmatter.
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// Why the text a store keeps could not be made from bytes in a form.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FormError {
    /// The bytes did not read in their form.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// The document read did not write in arky's form.
    #[error(transparent)]
    Encode(#[from] EncodeError),
}

/// The text a store keeps for a document, and the files it keeps beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoredBundle {
    /// The bytes of each file, by its path from the document's folder.
    pub files: BTreeMap<String, Vec<u8>>,
    /// The document in arky's form.
    pub text: String,
}

/// The bytes of `bundle` in `form`. `options` names the syntax of a valid
/// frontmatter in the markdown and zip forms.
///
/// Markdown and JSON hold each file as a data URI in its embed, and zip holds
/// `index.md` and each file at its path. The binary form holds each file's raw bytes. JSON and
/// binary leave `options` unused.
///
/// # Errors
///
/// The writer's error for the markdown form, `serde_json`'s or rkyv's for
/// the others, and the bundle's for a tree it cannot embed or a zip it cannot
/// write.
///
/// # Examples
///
/// ```rust
/// use std::collections::BTreeMap;
///
/// use arky::bundle::{BundledDocument, Missing};
/// use arky::forms::{Form, encode};
/// use arky::parser::parse;
/// use arky::writer::WriteOptions;
///
/// let document = parse("# Recipe\n\n![Loaf](images/loaf.gif)\n").document;
/// let files = BTreeMap::from([("images/loaf.gif".to_owned(), b"GIF89a".to_vec())]);
/// let bundle = BundledDocument::new(document, files, Missing::Refuse)?;
/// let markdown = encode(&bundle, Form::Markdown, &WriteOptions::default())?;
/// assert_eq!(
///     String::from_utf8(markdown)?,
///     "# Recipe\n\n![Loaf](data:image/gif;name=images/loaf.gif;base64,R0lGODlh)\n",
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
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
/// in place through [`stored`]. Markdown or JSON read on its own holds no file, so a relative
/// embed fails under [`Missing::Refuse`]; [`Missing::Keep`] keeps its target.
///
/// # Errors
///
/// [`DecodeError::NotUtf8`] naming the offset of the first byte that is not, a
/// JSON error, a header that is not arky's or names another version, an
/// rkyv error, and the bundle's error.
///
/// # Examples
///
/// ```rust
/// use arky::bundle::Missing;
/// use arky::forms::{Form, decode};
///
/// let report = b"# Report\n\n![Logo](media/logo.png)\n";
/// let refused = decode(report, Form::Markdown, Missing::Refuse).expect_err("no file is given");
/// assert_eq!(
///     refused.to_string(),
///     "index.md names media/logo.png, and the bundle does not hold it",
/// );
/// let kept = decode(report, Form::Markdown, Missing::Keep)?;
/// assert!(kept.files().is_empty());
/// # Ok::<(), arky::forms::DecodeError>(())
/// ```
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

/// The text a store keeps for `bytes` in `form`, arky's form with a valid
/// frontmatter in the syntax `options` names, and the files beside it.
///
/// `missing` decides an embed with no file. JSON and binary are written, read
/// and written again; binary is read in place with `write_archived`.
///
/// # Errors
///
/// The errors of [`decode`] and [`encode`].
///
/// # Examples
///
/// ```rust
/// use arky::bundle::Missing;
/// use arky::forms::{Form, stored};
/// use arky::writer::WriteOptions;
///
/// let options = WriteOptions::default();
/// let kept = stored(b"* flour\n* yeast\n", Form::Markdown, &options, Missing::Refuse)?;
/// assert_eq!(kept.text, "- flour\n- yeast\n");
/// assert!(kept.files.is_empty());
/// # Ok::<(), arky::forms::FormError>(())
/// ```
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
