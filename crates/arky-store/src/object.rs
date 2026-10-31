//! Files kept as objects in one bucket, through os-shim's `Bucket`.
//!
//! This module needs the `minio` feature. [`ObjectStore`] works over any
//! os-shim [`Bucket`]: `MinioBucket` for a `MinIO` server, or [`MemoryBucket`]
//! in tests, which reaches no server.
#![cfg_attr(
    feature = "minio",
    doc = "\nWith `minio`, [`ObjectConfig`] says where a server's bucket is, and [`ObjectStore::open`] builds the store over it through [`MinioBucket`]."
)]

use alloc::collections::BTreeMap;
use core::fmt;
use core::future;
use core::str;
use core::time::Duration;
use std::env::VarError;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use os_shim::System;
use os_shim::bucket::{Bucket, Condition, Depth, Guarantee, MemoryBucket, RevisionToken};
#[cfg(feature = "minio")]
use os_shim::bucket::{MinioBucket, MinioConfig};
use os_shim::error::{ErrorKind, StorageError};

use crate::lock::{self, Found, LockSettings, Locks};
use crate::{
    Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored, no_deletions,
    read_at_path,
};

/// The longest key a bucket takes, in bytes.
const LONGEST_KEY: usize = 1024;

/// Creates a bucket when it is missing. A bucket that exists is left as it is.
pub trait EnsureBucket {
    /// Creates the bucket.
    fn ensure_bucket(&self) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// Files in one bucket, under an optional key prefix.
///
/// A delete's precondition is checked by a request of its own before the
/// delete over `MinioBucket`, because `MinIO` ignores `If-Match` on a delete.
///
/// With the prefix `vault`, the file `notes/a.md` is the object
/// `vault/notes/a.md`; with none, it is the object `notes/a.md`. A key holds
/// at most 1024 bytes. A revision is the object's `ETag`, as the bucket gives
/// it. A precondition goes to the bucket with the put, as
/// `Condition::Absent` or `Condition::Matches`, so the check and the write are
/// one step. A save of a document and its files holds a lock object beside the
/// document (see [`lock`]), and the store's `system` gives the clock that
/// lock's age counts on.
///
/// ```rust
/// use arky_store::object::ObjectStore;
/// use arky_store::{Precondition, Store as _, StorePath};
/// use os_shim::bucket::{Bucket as _, MemoryBucket};
/// use os_shim::mock::MemorySystem;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let system = MemorySystem::new();
///     let bucket = MemoryBucket::default();
///     let store = ObjectStore::new(bucket.clone(), Some(StorePath::new("vault")?), &system);
///     let path = StorePath::new("notes/a.md")?;
///
///     let written = store.write(&path, b"# A\n", &Precondition::Absent).await?;
///     let (_bytes, revision) = bucket.get("/vault/notes/a.md".as_ref()).await?;
///     assert_eq!(revision.as_str(), written.0);
///     Ok(())
/// }
/// ```
pub struct ObjectStore<'system, B> {
    bucket: B,
    lock: LockSettings,
    prefix: Option<StorePath>,
    system: &'system dyn System,
}

impl<'system, B: Bucket> ObjectStore<'system, B> {
    /// The path of each object under `prefix`, locks included, sorted.
    async fn all_paths(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let root = self
            .prefix
            .as_ref()
            .map_or_else(|| PathBuf::from("/"), |store| bucket_path(store.as_str()));
        let listed = prefix.map_or_else(|| root.clone(), |folder| root.join(folder.as_str()));
        let entries = match self.bucket.list(&listed, Depth::All).await {
            Ok(entries) => entries,
            Err(error) if missing(&error) => Vec::new(),
            Err(error) => {
                return Err(StoreError::Backend(prefix.map_or_else(
                    || error.to_string(),
                    |folder| format!("{folder}: {error}"),
                )));
            }
        };
        let mut paths = entries
            .iter()
            .filter(|entry| entry.is_file)
            .map(|entry| {
                let relative = entry.path.strip_prefix(&root).ok().and_then(Path::to_str);
                relative.map_or_else(
                    || Err(StoreError::InvalidPath(entry.path.display().to_string())),
                    StorePath::new,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort_unstable();
        Ok(paths)
    }

    /// The key of the object that holds the file at `path`.
    fn key(&self, path: &StorePath) -> String {
        self.prefix.as_ref().map_or_else(
            || path.as_str().to_owned(),
            |prefix| format!("{prefix}/{path}"),
        )
    }

    /// How long the lock of a save lasts and how long a save waits on another,
    /// `LockSettings::default()` by default.
    ///
    /// The same settings decide which locks `remove_expired_locks` finds
    /// expired.
    #[must_use]
    pub fn locking(self, lock: LockSettings) -> Self {
        Self { lock, ..self }
    }

    /// A store over `bucket`, its keys under `prefix`, its clock `system`'s.
    ///
    /// It sends no request. Pass `MemoryBucket` and `MemorySystem` for a store
    /// that needs no server and no real clock.
    #[must_use]
    pub fn new(bucket: B, prefix: Option<StorePath>, system: &'system dyn System) -> Self {
        Self {
            bucket,
            lock: LockSettings::default(),
            prefix,
            system,
        }
    }

    /// The bucket path of the object that holds the file at `path`.
    fn place(&self, path: &StorePath) -> PathBuf {
        bucket_path(&self.key(path))
    }

    /// The bucket's put, after the parent folder is made on a bucket that keeps folders.
    async fn put(
        &self,
        path: &StorePath,
        bytes: &[u8],
        condition: &Condition,
    ) -> Result<RevisionToken, StorageError> {
        let place = self.place(path);
        if self
            .bucket
            .guarantees()
            .contains(Guarantee::EmptyFoldersExist)
            && let Some(parent) = place.parent()
        {
            self.bucket.create_folder(parent).await?;
        }
        self.bucket.put(&place, bytes, condition).await
    }

    /// The same store, its keys under `folder` inside the current prefix.
    #[must_use]
    pub fn within(self, folder: &StorePath) -> Self {
        // Two store paths joined by `/` are a store path.
        let prefix = self.prefix.as_ref().map_or_else(
            || folder.clone(),
            |prefix| StorePath(format!("{prefix}/{folder}")),
        );
        Self {
            prefix: Some(prefix),
            ..self
        }
    }
}

#[cfg(feature = "minio")]
impl<'system> ObjectStore<'system, MinioBucket> {
    /// Creates the store's bucket when it does not exist, and leaves a bucket
    /// that exists as it is.
    ///
    /// It sends a HEAD on the bucket, and a create only when that answers 404.
    /// The keys need the right to list the bucket, and to create it when it is
    /// missing.
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` when the server cannot be reached, or refuses to
    /// say whether the bucket exists or to create it.
    pub async fn ensure(&self) -> Result<(), StoreError> {
        self.bucket
            .ensure()
            .await
            .map_err(|error| StoreError::Backend(error.to_string()))
    }

    /// A store over the `MinIO` server `config` names, its keys read from the
    /// environment.
    ///
    /// It reads `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` through
    /// `system`, and sends no request: a server that cannot be reached fails
    /// the first read or write, naming its path. Call [`ensure`](Self::ensure)
    /// to create the bucket before the first write. Run the store's futures on
    /// tokio. Over `RealSystem` the keys come from the process's environment,
    /// and over `MemorySystem` from its `with_env`:
    ///
    /// ```rust
    /// use arky_store::object::{ObjectConfig, ObjectStore};
    /// use arky_store::{StoreError, StorePath};
    /// use os_shim::mock::MemorySystem;
    ///
    /// let config = ObjectConfig::new(
    ///     "http://127.0.0.1:9000".to_owned(),
    ///     "us-east-1".to_owned(),
    ///     "notes".to_owned(),
    ///     Some(StorePath::new("vault")?),
    /// );
    /// let system = MemorySystem::new()
    ///     .with_env("AWS_ACCESS_KEY_ID", "notes-app")?
    ///     .with_env("AWS_SECRET_ACCESS_KEY", "a-secret")?;
    /// assert!(ObjectStore::open(&system, config.clone()).is_ok());
    ///
    /// let no_keys = MemorySystem::new();
    /// assert_eq!(
    ///     ObjectStore::open(&no_keys, config).err(),
    ///     Some(StoreError::Backend(
    ///         "AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key".to_owned()
    ///     )),
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// `Backend` naming the variable and what it holds when `AWS_ACCESS_KEY_ID`
    /// or `AWS_SECRET_ACCESS_KEY` is not set or is not UTF-8. The message
    /// never shows the variable's value.
    pub fn open(system: &'system dyn System, config: ObjectConfig) -> Result<Self, StoreError> {
        let access_key = variable(system, "AWS_ACCESS_KEY_ID", "the MinIO access key")?;
        let secret_key = variable(system, "AWS_SECRET_ACCESS_KEY", "the MinIO secret key")?;
        let minio = MinioConfig::new(
            config.endpoint,
            config.region,
            config.bucket,
            access_key,
            secret_key,
        );
        Ok(Self::new(MinioBucket::new(minio), config.prefix, system))
    }
}

impl<B: fmt::Debug> fmt::Debug for ObjectStore<'_, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectStore")
            .field("bucket", &self.bucket)
            .field("lock", &self.lock)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// A lock is an object holding `<guid> <seconds since 1970-01-01 UTC>`: an
/// object has no modification time, so it holds the time it was taken.
impl<B: Bucket> Locks for ObjectStore<'_, B> {
    async fn create(&self, lock: &StorePath, guid: &str) -> Result<bool, StoreError> {
        let taken = self
            .system
            .now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|error| StoreError::Backend(format!("{lock}: {error}")))?;
        let bytes = format!("{guid} {}", taken.as_secs());
        match self.put(lock, bytes.as_bytes(), &Condition::Absent).await {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::AlreadyExists | ErrorKind::PreconditionFailed
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(failure(lock, &error)),
        }
    }

    async fn find(&self, lock: &StorePath) -> Result<Option<Found>, StoreError> {
        let (bytes, tag) = match self.bucket.get(&self.place(lock)).await {
            Ok(object) => object,
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(failure(lock, &error)),
        };
        let (holder, taken) = held_by(&bytes).unwrap_or((String::new(), SystemTime::UNIX_EPOCH));
        Ok(Some(Found {
            holder,
            taken,
            tag: tag.as_str().as_bytes().to_vec(),
        }))
    }

    async fn remove_if(&self, lock: &StorePath, found: &Found) -> Result<bool, StoreError> {
        let token = RevisionToken::new(String::from_utf8_lossy(&found.tag));
        match self
            .bucket
            .delete(&self.place(lock), &Condition::Matches(token))
            .await
        {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::PreconditionFailed || missing(&error) => {
                Ok(false)
            }
            Err(error) => Err(failure(lock, &error)),
        }
    }

    /// The object's `ETag`, from a request that sends no bytes back.
    async fn revision(&self, path: &StorePath) -> Result<Option<Revision>, StoreError> {
        match self.bucket.head(&self.place(path)).await {
            Ok(head) if head.is_file => Ok(head
                .revision
                .map(|token| Revision(token.as_str().to_owned()))),
            Ok(_) => Ok(None),
            Err(error) if missing(&error) => Ok(None),
            Err(error) => Err(failure(path, &error)),
        }
    }

    fn system(&self) -> &dyn System {
        self.system
    }
}

impl<B: Bucket + EnsureBucket> Store for ObjectStore<'_, B> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            atomic_preconditions: true,
        }
    }

    /// Deletes the object of `path` when `precondition` holds.
    ///
    /// Under `Precondition::None` or `Precondition::Absent`, a delete where no
    /// file is succeeds, as the bucket answers it.
    async fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        self.bucket
            .delete(&self.place(path), &condition(precondition))
            .await
            .map_err(|error| failure(path, &error))
    }

    /// Creates the store's bucket when it is missing.
    fn ensure_container(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.bucket.ensure_bucket()
    }

    fn finish_deletions(&self) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        no_deletions()
    }

    /// Every file under `prefix`, sorted, with lock objects left out. One key
    /// under the store's prefix that is not a store path fails the whole
    /// listing with `StoreError::InvalidPath`.
    async fn list(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let mut paths = self.all_paths(prefix).await?;
        paths.retain(|path| !path.as_str().ends_with(lock::SUFFIX));
        Ok(paths)
    }

    async fn read(&self, path: &StorePath) -> Result<Stored, StoreError> {
        let (bytes, token) = self
            .bucket
            .get(&self.place(path))
            .await
            .map_err(|error| failure(path, &error))?;
        Ok(Stored {
            bytes,
            revision: Revision(token.as_str().to_owned()),
        })
    }

    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send {
        read_at_path(self, file)
    }

    /// Removes each expired lock under `under` (the whole store when `None`),
    /// each one only while it is still the lock found, and gives the path of
    /// each lock it removed, sorted.
    ///
    /// It clears the locks left behind by saves that stopped. Only objects whose
    /// keys end with [`lock::SUFFIX`] count as locks, and a lock has expired
    /// once it is older than the store's [`LockSettings::expires`].
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` when the store cannot be listed or a lock cannot
    /// be read or removed, and `StoreError::InvalidPath` for a key under
    /// `under` that is not a store path.
    async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        let mut locks = self.all_paths(under).await?;
        locks.retain(|path| path.as_str().ends_with(lock::SUFFIX));
        lock::remove_expired(self, &locks, self.lock).await
    }

    /// Puts `bytes` at the key of `path` when `precondition` holds, and gives
    /// the object's `ETag`. A key longer than 1024 bytes, prefix included, is
    /// `StoreError::InvalidPath`.
    async fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        let key = self.key(path);
        if key.len() > LONGEST_KEY {
            return Err(StoreError::InvalidPath(path.as_str().to_owned()));
        }
        let token = self
            .put(path, bytes, &condition(precondition))
            .await
            .map_err(|error| {
                // A put with `If-Match` where no object is answers 404 on RustFS.
                let unmatched = matches!(precondition, Precondition::Matches(_)) && missing(&error);
                if unmatched {
                    StoreError::Conflict(path.clone())
                } else {
                    failure(path, &error)
                }
            })?;
        Ok(Revision(token.as_str().to_owned()))
    }

    /// Holds the lock object of `path` from before the first file to after the
    /// document. The lock's key, which is the document's key followed by
    /// `.arky-lock`, must fit in 1024 bytes too, or the save is
    /// `StoreError::InvalidPath`.
    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        write_locked(self, path, text, files, precondition)
    }
}

/// The bucket in memory has nothing to create.
impl EnsureBucket for MemoryBucket {
    fn ensure_bucket(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
        future::ready(Ok(()))
    }
}

#[cfg(feature = "minio")]
impl EnsureBucket for MinioBucket {
    async fn ensure_bucket(&self) -> Result<(), StoreError> {
        self.ensure()
            .await
            .map_err(|error| StoreError::Backend(error.to_string()))
    }
}

/// Where the bucket is: the `MinIO` server, its region, the bucket, and the
/// key prefix.
#[cfg(feature = "minio")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObjectConfig {
    /// The bucket's name.
    pub bucket: String,
    /// The server's address, such as `http://127.0.0.1:9000`; required, no
    /// default.
    pub endpoint: String,
    /// The key prefix every object of the store sits under, or `None` for the
    /// bucket's root.
    pub prefix: Option<StorePath>,
    /// The region the requests are signed for, such as `us-east-1`.
    pub region: String,
}

#[cfg(feature = "minio")]
impl ObjectConfig {
    /// The bucket `bucket` on the server at `endpoint` in `region`, its
    /// objects under `prefix`.
    ///
    /// ```rust
    /// use arky_store::StorePath;
    /// use arky_store::object::ObjectConfig;
    ///
    /// let config = ObjectConfig::new(
    ///     "http://127.0.0.1:9000".to_owned(),
    ///     "us-east-1".to_owned(),
    ///     "notes".to_owned(),
    ///     Some(StorePath::new("vault")?),
    /// );
    /// assert_eq!(config.bucket, "notes");
    /// assert_eq!(config.prefix, Some(StorePath::new("vault")?));
    /// # Ok::<(), arky_store::StoreError>(())
    /// ```
    #[must_use]
    pub const fn new(
        endpoint: String,
        region: String,
        bucket: String,
        prefix: Option<StorePath>,
    ) -> Self {
        Self {
            bucket,
            endpoint,
            prefix,
            region,
        }
    }
}

/// The bucket's condition for `precondition`, a revision being the entity tag.
fn condition(precondition: &Precondition) -> Condition {
    match precondition {
        Precondition::Absent => Condition::Absent,
        Precondition::Matches(revision) => {
            Condition::Matches(RevisionToken::new(revision.0.clone()))
        }
        Precondition::None => Condition::None,
    }
}

/// The GUID and the time a lock holds, or `None` when its bytes do not read
/// as `<guid> <seconds>`.
fn held_by(bytes: &[u8]) -> Option<(String, SystemTime)> {
    let (guid, seconds) = str::from_utf8(bytes).ok()?.split_once(' ')?;
    let taken = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds.parse().ok()?))?;
    Some((guid.to_owned(), taken))
}

/// The bucket path of the object at `key`; os-shim's buckets take absolute paths.
fn bucket_path(key: &str) -> PathBuf {
    PathBuf::from(format!("/{key}"))
}

/// `NotFound` or `Conflict` of `path` when `error` says so, else `Backend`
/// naming `path`.
fn failure(path: &StorePath, error: &StorageError) -> StoreError {
    match error.kind() {
        ErrorKind::AlreadyExists | ErrorKind::PreconditionFailed => {
            StoreError::Conflict(path.clone())
        }
        ErrorKind::NotADirectory | ErrorKind::NotFound => StoreError::NotFound(path.clone()),
        ErrorKind::InvalidData
        | ErrorKind::InvalidInput
        | ErrorKind::IsADirectory
        | ErrorKind::Other
        | ErrorKind::PermissionDenied
        | ErrorKind::Unavailable
        | ErrorKind::Unsupported
        | _ => StoreError::Backend(format!("{path}: {error}")),
    }
}

/// Whether `error` says no object is at the path: nothing there, or an object
/// where a folder of the path should be.
const fn missing(error: &StorageError) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// The value of the variable `name`, which holds `holds`; the error names the
/// variable, never its value.
fn variable(system: &dyn System, name: &str, holds: &str) -> Result<String, StoreError> {
    system.env_var(name).map_err(|error| {
        let what = match error {
            VarError::NotPresent => "is not set",
            VarError::NotUnicode(_) => "is not UTF-8",
        };
        StoreError::Backend(format!("{name} {what}: it holds {holds}"))
    })
}

/// `lock::write_with_embedded` over `store`, as the file store saves, once the
/// key of the lock of `path` fits: past `LONGEST_KEY` it is `InvalidPath` of
/// `path`.
async fn write_locked<B>(
    store: &ObjectStore<'_, B>,
    path: &StorePath,
    text: &str,
    files: &BTreeMap<StorePath, Vec<u8>>,
    precondition: &Precondition,
) -> Result<Revision, StoreError>
where
    B: Bucket + EnsureBucket,
{
    if store.key(&lock::lock_of(path)?).len() > LONGEST_KEY {
        return Err(StoreError::InvalidPath(path.as_str().to_owned()));
    }
    lock::write_with_embedded(store, path, text, files, precondition, store.lock).await
}

#[cfg(test)]
mod tests;
