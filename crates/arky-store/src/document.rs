//! A document over any store: stored in arky's form, with each file it
//! embeds beside it.

use alloc::collections::BTreeMap;
use core::str::from_utf8;
use std::path::Path;

use arky::bundle::{BundledDocument, BundledDocumentError, Missing, embedded_paths};
use arky::forms::{DecodeError, EncodeError, Form, FormError, stored};
use arky::parser::ast::RawDocument;
use arky::parser::parse;
use arky::writer::{self, WriteOptions};

use crate::{Precondition, Revision, Store, StoreError, StorePath};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentError {
    #[error("{path}: {error}")]
    Form { path: StorePath, error: FormError },
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
/// `missing` decides an embed whose file the store does not hold.
///
/// # Errors
///
/// The store's error, or the bundle's naming the path.
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
/// # Errors
///
/// The form's error naming the path, or the store's.
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
/// `options` names, and each file of `bundle` beside it, and gives the
/// document's new revision.
///
/// `precondition` applies to the document only.
///
/// # Errors
///
/// The writer's error naming the path, or the store's.
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
