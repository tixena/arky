//! A document over any store: stored in arky's form, with each file it
//! embeds beside it.
//!
//! [`get`] reads a document and every file it embeds into an
//! [`arky::bundle::BundledDocument`]. [`put`] stores a document given as bytes
//! in any [`arky::forms::Form`], and [`put_bundle`] stores a bundle. A file
//! sits at its path from the document's folder: the `media/logo.png` that
//! `notes/report.md` embeds is kept at `notes/media/logo.png`. Only image
//! embeds travel, written `![alt](path)` or `![[name]]`, an embedded note's
//! own embeds included.

use alloc::collections::BTreeMap;
use core::str::from_utf8;
use std::path::Path;

use arky::bundle::{BundledDocument, BundledDocumentError, Missing, embedded_paths};
use arky::forms::{DecodeError, EncodeError, Form, FormError, stored};
use arky::parser::ast::RawDocument;
use arky::parser::parse;
use arky::writer::{self, WriteOptions};

use crate::{Precondition, Revision, Store, StoreError, StorePath};

/// Why [`get`], [`put`] or [`put_bundle`] failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// The document at `path`, or a note it embeds, does not read or write in
    /// its form, for the reason `error` gives: `notes/a.md: not UTF-8 at byte 1`.
    #[error("{path}: {error}")]
    Form { path: StorePath, error: FormError },
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl DocumentError {
    fn form<E>(path: &StorePath, error: E) -> Self
    where
        E: Into<FormError>,
    {
        Self::Form {
            path: path.clone(),
            error: error.into(),
        }
    }
}

/// The document at `path` and every file it embeds, an embedded note's own
/// embeds included, read beside it.
///
/// `missing` decides an embed whose file the store does not hold. An embed
/// whose path beside the document is not a store path names no file.
///
/// # Errors
///
/// `DocumentError::Store` when no document is at `path` or the store fails.
/// `DocumentError::Form` naming `path` when the document or a note it embeds
/// is not UTF-8, or when an embed has no file and `missing` is
/// `Missing::Refuse`.
pub async fn get<S>(
    store: &S,
    path: &StorePath,
    missing: Missing,
) -> Result<BundledDocument, DocumentError>
where
    S: Store + Sync,
{
    let document = read_tree(store, path).await?;
    let mut files = BTreeMap::new();
    let mut pending = vec![(String::new(), document.clone())];
    while let Some((folder, text)) = pending.pop() {
        let named = embedded_paths(&text, &folder)
            .map_err(|error| DocumentError::form(path, DecodeError::from(error)))?;
        for (_, name) in named {
            if files.contains_key(&name) {
                continue;
            }
            let file = match path.beside(&name) {
                Ok(file) => file,
                // No store holds a file at a path that is not a store path.
                Err(StoreError::InvalidPath(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            match store.read_embedded(path, &file).await {
                Ok(bytes) => {
                    if is_note(&name) {
                        let note = note_text(&name, &bytes)
                            .map_err(|error| DocumentError::form(path, DecodeError::from(error)))?;
                        pending.push((parent_of(&name).to_owned(), parse(note).document));
                    }
                    files.insert(name, bytes);
                }
                // `BundledDocument::new` refuses the embed unless `missing` is `Keep`.
                Err(StoreError::NotFound(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    BundledDocument::new(document, files, missing)
        .map_err(|error| DocumentError::form(path, DecodeError::from(error)))
}

/// Stores at `path` arky's form of the document `bytes` hold in `form`, and
/// each file it embeds beside it, and gives the document's new revision.
///
/// `precondition` applies to the document only, and `missing` decides an
/// embed with no file.
///
/// The store keeps arky's form, not the bytes given: `* item` is kept as
/// `- item`, a valid frontmatter in the syntax `options` names, and each data
/// URI as a file beside the document. Here a post and its picture arrive as a
/// zip:
///
/// ```rust
/// use std::collections::BTreeMap;
/// use std::path::PathBuf;
///
/// use arky::bundle::{BundledDocument, Missing};
/// use arky::forms::{Form, encode};
/// use arky::parser::parse;
/// use arky::writer::WriteOptions;
/// use arky_store::file::FileStore;
/// use arky_store::{Precondition, Store as _, StorePath, document};
/// use os_shim::mock::MemorySystem;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let system = MemorySystem::new();
///     let store = FileStore::new(PathBuf::from("/blog"), &system);
///     let path = StorePath::new("posts/bread.md")?;
///     let options = WriteOptions::default();
///
///     let text = "# Bread\n\n![The loaf](media/loaf.png)\n";
///     let files = BTreeMap::from([("media/loaf.png".to_owned(), b"not a real picture".to_vec())]);
///     let bundle = BundledDocument::new(parse(text).document, files, Missing::Refuse)?;
///     let zip = encode(&bundle, Form::Zip, &options)?;
///
///     document::put(&store, &path, Form::Zip, &zip, &options, &Precondition::Absent, Missing::Refuse)
///         .await?;
///     assert_eq!(store.read(&path).await?.bytes, text.as_bytes());
///     let picture = StorePath::new("posts/media/loaf.png")?;
///     assert_eq!(store.read(&picture).await?.bytes, b"not a real picture");
///     Ok(())
/// }
/// ```
///
#[cfg_attr(
    feature = "binary-format",
    doc = r#"With the `binary-format` feature, `form` may be `Form::Binary`, and the
document's tree is read from the archive in place:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use arky::bundle::{BundledDocument, Missing};
use arky::forms::{Form, encode};
use arky::parser::parse;
use arky::writer::WriteOptions;
use arky_store::file::FileStore;
use arky_store::{Precondition, Store as _, StorePath, document};
use os_shim::mock::MemorySystem;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::new();
    let store = FileStore::new(PathBuf::from("/notes"), &system);
    let path = StorePath::new("todo.md")?;
    let options = WriteOptions::default();

    let bundle = BundledDocument::new(parse("* item\n").document, BTreeMap::new(), Missing::Refuse)?;
    let binary = encode(&bundle, Form::Binary, &options)?;
    document::put(&store, &path, Form::Binary, &binary, &options, &Precondition::None, Missing::Refuse)
        .await?;
    assert_eq!(store.read(&path).await?.bytes, b"- item\n");
    Ok(())
}
```
"#
)]
///
/// # Errors
///
/// `DocumentError::Form` naming `path` when `bytes` do not hold a document in
/// `form`, or when an embed has no file and `missing` is `Missing::Refuse`.
/// `DocumentError::Store` when the store refuses or fails: `Conflict` when
/// `precondition` does not hold, `InvalidPath` for a file whose path beside
/// the document is not a store path.
pub async fn put<S>(
    store: &S,
    path: &StorePath,
    form: Form,
    bytes: &[u8],
    options: &WriteOptions,
    precondition: &Precondition,
    missing: Missing,
) -> Result<Revision, DocumentError>
where
    S: Store + Sync,
{
    let kept =
        stored(bytes, form, options, missing).map_err(|error| DocumentError::form(path, error))?;
    let files = beside_all(path, kept.files)?;
    Ok(store
        .write_with_embedded(path, &kept.text, &files, precondition)
        .await?)
}

/// Stores at `path` the document of `bundle`, its frontmatter in the syntax
/// `options` names when it is valid, and each file of `bundle` beside it, and
/// gives the document's new revision.
///
/// `precondition` applies to the document only.
///
/// # Errors
///
/// `DocumentError::Form` naming `path` when the document does not write in
/// the syntax `options` names, such as a null value in TOML.
/// `DocumentError::Store` when the store refuses or fails: `Conflict` when
/// `precondition` does not hold, `InvalidPath` for a file whose path beside
/// the document is not a store path.
pub async fn put_bundle<S>(
    store: &S,
    path: &StorePath,
    bundle: &BundledDocument,
    options: &WriteOptions,
    precondition: &Precondition,
) -> Result<Revision, DocumentError>
where
    S: Store + Sync,
{
    let text = writer::write(bundle.document(), options)
        .map_err(|error| DocumentError::form(path, EncodeError::from(error)))?;
    let mut files = BTreeMap::new();
    for name in bundle.files() {
        if let Some(bytes) = bundle.file(name) {
            files.insert(path.beside(name)?, bytes.to_vec());
        }
    }
    Ok(store
        .write_with_embedded(path, &text, &files, precondition)
        .await?)
}

/// Each file of `files`, by its path in the bundle, at its path beside the
/// document at `path`.
fn beside_all(
    path: &StorePath,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<BTreeMap<StorePath, Vec<u8>>, StoreError> {
    files
        .into_iter()
        .map(|(name, bytes)| Ok((path.beside(&name)?, bytes)))
        .collect()
}

/// Whether the file at `name` is a note: its extension is `md`, in any case.
pub(crate) fn is_note(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// The text of the note at `name` in the bundle, or the offset of the first
/// byte of `bytes` that is not UTF-8.
fn note_text<'bytes>(name: &str, bytes: &'bytes [u8]) -> Result<&'bytes str, BundledDocumentError> {
    from_utf8(bytes).map_err(|error| BundledDocumentError::NotUtf8 {
        path: name.to_owned(),
        offset: error.valid_up_to(),
    })
}

pub(crate) fn parent_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(folder, _)| folder)
}

async fn read_tree<S>(store: &S, path: &StorePath) -> Result<RawDocument, DocumentError>
where
    S: Store + Sync,
{
    let bytes = store.read(path).await?.bytes;
    let text = from_utf8(&bytes).map_err(|error| {
        DocumentError::form(
            path,
            DecodeError::NotUtf8 {
                offset: error.valid_up_to(),
            },
        )
    })?;
    Ok(parse(text).document)
}

#[cfg(test)]
mod tests;
