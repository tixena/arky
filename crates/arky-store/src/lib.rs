//! Where arky keeps documents: the storage interface and its stores.

extern crate alloc;

#[cfg(feature = "mongodb")]
tixschema::decode_with_foundation!();
#[cfg(feature = "mongodb")]
tixschema::mongodb_foundation!();

pub mod any;
pub mod document;
pub mod file;
pub mod lock;
#[cfg(feature = "mongodb")]
pub mod mongo;
#[cfg(feature = "minio")]
pub mod object;

use alloc::collections::BTreeMap;
use core::{fmt, future};

/// A file's place in a store: parts joined by `/`, none empty, none `.` or
/// `..`, no `\`, with no `/` at either end.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorePath(String);

impl StorePath {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path of `name`, a path inside a bundle, from the folder of `self`:
    /// `notes/report.md` and `media/logo.png` give `notes/media/logo.png`.
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidPath` when the path breaks one of the rules above.
    pub fn beside(&self, name: &str) -> Result<Self, StoreError> {
        self.0.rsplit_once('/').map_or_else(
            || Self::new(name),
            |(folder, _)| Self::new(&format!("{folder}/{name}")),
        )
    }

    /// # Errors
    ///
    /// `StoreError::InvalidPath` when `text` breaks one of the rules above.
    pub fn new(text: &str) -> Result<Self, StoreError> {
        let valid = text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains('\\'));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(StoreError::InvalidPath(text.to_owned()))
        }
    }
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a store gives with the bytes it read, and takes back for a write that
/// waits for nothing else to have changed the file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Revision(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stored {
    pub bytes: Vec<u8>,
    pub revision: Revision,
}

/// What must hold for a write or a delete to go ahead.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precondition {
    /// No file is at the path.
    Absent,
    /// The file at the path still has this revision.
    Matches(Revision),
    /// Nothing: the write replaces whatever is there.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The store failed: the file system, the network, the server.
    #[error("{0}")]
    Backend(String),
    #[error("{0}: the file changed, or a file is already there")]
    Conflict(StorePath),
    #[error("`{0}` is not a store path")]
    InvalidPath(String),
    #[error("{0}: no file at this path")]
    NotFound(StorePath),
    #[error("this store cannot {0}")]
    Unsupported(&'static str),
}

/// What a store can do that not every store can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// A precondition is checked and the write done as one step.
    pub atomic_preconditions: bool,
}

pub trait Store {
    fn capabilities(&self) -> Capabilities;

    fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Every file under `prefix`, or under the root with no prefix, at any
    /// depth, in byte order of their paths.
    fn list(
        &self,
        prefix: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send;

    fn read(&self, path: &StorePath) -> impl Future<Output = Result<Stored, StoreError>> + Send;

    /// The bytes of `file`, which the document at `_document` embeds. By
    /// default, the file at that path.
    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send
    where
        Self: Sync,
    {
        read_at_path(self, file)
    }

    /// Writes `bytes` whole: a reader sees the old file or the new one, never a
    /// part of either.
    fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send;

    /// Writes the document `text` at `path` and each file of `files`, which it
    /// embeds. By default, each file at its path, with no precondition, then the
    /// text under `precondition`; no file is deleted.
    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send
    where
        Self: Sync,
    {
        write_at_paths(self, path, text, files, precondition, || {
            future::ready(Ok(()))
        })
    }
}

/// What `Store::read_embedded` does by default: the bytes of the file at
/// `file`.
pub(crate) async fn read_at_path<S>(store: &S, file: &StorePath) -> Result<Vec<u8>, StoreError>
where
    S: Store + Sync + ?Sized,
{
    Ok(store.read(file).await?.bytes)
}

/// What `Store::write_with_embedded` does by default: each file of `files` at
/// its path, with no precondition, then `text` at `path` under `precondition`,
/// each write after `check` passes.
pub(crate) async fn write_at_paths<S, C, F>(
    store: &S,
    path: &StorePath,
    text: &str,
    files: &BTreeMap<StorePath, Vec<u8>>,
    precondition: &Precondition,
    check: C,
) -> Result<Revision, StoreError>
where
    S: Store + Sync + ?Sized,
    C: Fn() -> F + Send + Sync,
    F: Future<Output = Result<(), StoreError>> + Send,
{
    for (file, bytes) in files {
        check().await?;
        store.write(file, bytes, &Precondition::None).await?;
    }
    check().await?;
    store.write(path, text.as_bytes(), precondition).await
}

#[cfg(test)]
mod tests;
