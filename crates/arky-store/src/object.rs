//! Files kept as objects in one bucket, through os-shim's `Bucket`.
//!
//! This module needs the `minio` feature or the `s3` feature. [`ObjectStore`]
//! works over any os-shim [`Bucket`]: `MinioBucket` for a `MinIO` or other
//! S3-compatible server, `S3Bucket` for an AWS S3 bucket, or
//! [`MemoryBucket`](os_shim::bucket::MemoryBucket) in tests, which reaches no
//! server.
#![cfg_attr(
    feature = "minio",
    doc = "\nWith `minio`, [`ObjectConfig`] says where a server's bucket is, and [`ObjectStore::open`] builds the store over it through [`MinioBucket`]."
)]
#![cfg_attr(
    feature = "s3",
    doc = "\nWith `s3`, [`S3ObjectConfig`] says where an AWS S3 bucket is, and [`ObjectStore::open_s3`] builds the store over it through [`S3Bucket`]."
)]

use alloc::collections::BTreeMap;
use core::fmt;
use core::str;
use core::time::Duration;
use std::env::VarError;
use std::time::SystemTime;

use os_shim::System;
use os_shim::bucket::{Bucket, Condition, ObjectError, ObjectErrorKind};
#[cfg(feature = "minio")]
use os_shim::bucket::{MinioBucket, MinioConfig};
#[cfg(feature = "s3")]
use os_shim::bucket::{S3Bucket, S3Config};

use crate::lock::{self, Found, LockSettings, Locks};
use crate::{
    Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored, read_at_path,
};

/// The longest key S3 takes, in bytes.
const LONGEST_KEY: usize = 1024;

/// Files in one bucket, under an optional key prefix.
///
/// A delete's precondition is checked by a request of its own before the
/// delete over `MinioBucket`, because `MinIO` ignores `If-Match` on a delete,
/// and in one step with it over `S3Bucket`, which sends `If-Match` and lets
/// AWS check it.
///
/// With the prefix `vault`, the file `notes/a.md` is the object
/// `vault/notes/a.md`; with none, it is the object `notes/a.md`. A key holds
/// at most 1024 bytes. A revision is the object's `ETag`, as the bucket gives
/// it. A precondition goes to the bucket with the put, as
/// `Condition::Absent` or `Condition::ETag`, so the check and the write are
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
///     assert_eq!(bucket.list("").await?, ["vault/notes/a.md"]);
///     assert_eq!(bucket.get("vault/notes/a.md").await?.etag, written.0);
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
            .map_or_else(String::new, |store| format!("{store}/"));
        let listed = prefix.map_or_else(|| root.clone(), |folder| format!("{root}{folder}/"));
        let keys = self.bucket.list(&listed).await.map_err(|error| {
            StoreError::Backend(
                prefix.map_or_else(|| error.to_string(), |folder| format!("{folder}: {error}")),
            )
        })?;
        let mut paths = keys
            .iter()
            .filter(|key| !key.ends_with('/'))
            .map(|key| {
                key.strip_prefix(&root)
                    .map_or_else(|| Err(StoreError::InvalidPath(key.clone())), StorePath::new)
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
    pub async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        let mut locks = self.all_paths(under).await?;
        locks.retain(|path| path.as_str().ends_with(lock::SUFFIX));
        lock::remove_expired(self, &locks, self.lock).await
    }

    /// `lock::write_with_embedded`, as the file store saves, once the key of
    /// the lock of `path` fits: past `LONGEST_KEY` it is `InvalidPath` of `path`.
    async fn write_locked(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        if self.key(&lock::lock_of(path)?).len() > LONGEST_KEY {
            return Err(StoreError::InvalidPath(path.as_str().to_owned()));
        }
        lock::write_with_embedded(self, path, text, files, precondition, self.lock).await
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

#[cfg(feature = "s3")]
impl<'system> ObjectStore<'system, S3Bucket> {
    /// Creates the store's bucket when it does not exist: private, in the
    /// store's region, and encrypted by default with the KMS key
    /// `kms_key_arn` when one is given. A bucket that exists is left as it is.
    ///
    /// The keys need the right to list the bucket, and to create it and set
    /// its encryption when it is missing.
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` when AWS cannot be reached, or refuses to say
    /// whether the bucket exists, to create it or to set its encryption.
    pub async fn ensure(&self, kms_key_arn: Option<&str>) -> Result<(), StoreError> {
        self.bucket
            .ensure(kms_key_arn)
            .await
            .map_err(|error| StoreError::Backend(error.to_string()))
    }

    /// A store over the AWS S3 bucket `config` names, its keys read from the
    /// environment.
    ///
    /// It reads `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` through
    /// `system`, and sends no request. Run the store's futures on tokio.
    ///
    /// ```rust
    /// use arky_store::StoreError;
    /// use arky_store::object::{ObjectStore, S3ObjectConfig};
    /// use os_shim::mock::MemorySystem;
    ///
    /// let config = S3ObjectConfig::new("eu-west-1".to_owned(), "notes".to_owned(), None);
    /// let system = MemorySystem::new()
    ///     .with_env("AWS_ACCESS_KEY_ID", "notes-app")?
    ///     .with_env("AWS_SECRET_ACCESS_KEY", "a-secret")?;
    /// assert!(ObjectStore::open_s3(&system, config.clone()).is_ok());
    ///
    /// assert_eq!(
    ///     ObjectStore::open_s3(&MemorySystem::new(), config).err(),
    ///     Some(StoreError::Backend(
    ///         "AWS_ACCESS_KEY_ID is not set: it holds the AWS access key".to_owned()
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
    pub fn open_s3(
        system: &'system dyn System,
        config: S3ObjectConfig,
    ) -> Result<Self, StoreError> {
        let access_key = variable(system, "AWS_ACCESS_KEY_ID", "the AWS access key")?;
        let secret_key = variable(system, "AWS_SECRET_ACCESS_KEY", "the AWS secret key")?;
        let s3 = S3Config::new(config.region, config.bucket, access_key, secret_key);
        Ok(Self::new(S3Bucket::new(s3), config.prefix, system))
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
        match self
            .bucket
            .put(&self.key(lock), bytes.as_bytes(), &Condition::Absent)
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ObjectErrorKind::PreconditionFailed => Ok(false),
            Err(error) => Err(failure(lock, &error)),
        }
    }

    async fn find(&self, lock: &StorePath) -> Result<Option<Found>, StoreError> {
        let object = match self.bucket.get(&self.key(lock)).await {
            Ok(object) => object,
            Err(error) if error.kind() == ObjectErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(failure(lock, &error)),
        };
        let (holder, taken) =
            held_by(&object.bytes).unwrap_or((String::new(), SystemTime::UNIX_EPOCH));
        Ok(Some(Found {
            holder,
            taken,
            tag: object.etag.into_bytes(),
        }))
    }

    async fn remove_if(&self, lock: &StorePath, found: &Found) -> Result<bool, StoreError> {
        let tag = String::from_utf8_lossy(&found.tag).into_owned();
        match self
            .bucket
            .delete(&self.key(lock), &Condition::ETag(tag))
            .await
        {
            Ok(()) => Ok(true),
            Err(error)
                if matches!(
                    error.kind(),
                    ObjectErrorKind::PreconditionFailed | ObjectErrorKind::NotFound
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(failure(lock, &error)),
        }
    }

    /// The object's `ETag`, from a request that sends no bytes back.
    async fn revision(&self, path: &StorePath) -> Result<Option<Revision>, StoreError> {
        match self.bucket.head(&self.key(path)).await {
            Ok(head) => Ok(Some(Revision(head.etag))),
            Err(error) if error.kind() == ObjectErrorKind::NotFound => Ok(None),
            Err(error) => Err(failure(path, &error)),
        }
    }

    fn system(&self) -> &dyn System {
        self.system
    }
}

impl<B: Bucket> Store for ObjectStore<'_, B> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            atomic_preconditions: true,
        }
    }

    /// Deletes the object of `path` when `precondition` holds.
    ///
    /// Under `Precondition::None` or `Precondition::Absent`, a delete where no
    /// file is succeeds, as S3 answers it.
    async fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        self.bucket
            .delete(&self.key(path), &condition(precondition))
            .await
            .map_err(|error| failure(path, &error))
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
        let object = self
            .bucket
            .get(&self.key(path))
            .await
            .map_err(|error| failure(path, &error))?;
        Ok(Stored {
            bytes: object.bytes,
            revision: Revision(object.etag),
        })
    }

    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send {
        read_at_path(self, file)
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
        let etag = self
            .bucket
            .put(&key, bytes, &condition(precondition))
            .await
            .map_err(|error| {
                // A put with `If-Match` where no object is answers 404 on RustFS.
                let unmatched = matches!(precondition, Precondition::Matches(_))
                    && error.kind() == ObjectErrorKind::NotFound;
                if unmatched {
                    StoreError::Conflict(path.clone())
                } else {
                    failure(path, &error)
                }
            })?;
        Ok(Revision(etag))
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
        self.write_locked(path, text, files, precondition)
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

/// Where an AWS S3 bucket is: its region, the bucket, and the key prefix.
#[cfg(feature = "s3")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct S3ObjectConfig {
    /// The bucket's name.
    pub bucket: String,
    /// The key prefix every object of the store sits under, or `None` for the
    /// bucket's root.
    pub prefix: Option<StorePath>,
    /// The bucket's AWS region, such as `eu-west-1`.
    pub region: String,
}

#[cfg(feature = "s3")]
impl S3ObjectConfig {
    /// The bucket `bucket` in the AWS region `region`, its objects under
    /// `prefix`.
    ///
    /// ```rust
    /// use arky_store::StorePath;
    /// use arky_store::object::S3ObjectConfig;
    ///
    /// let config = S3ObjectConfig::new(
    ///     "eu-west-1".to_owned(),
    ///     "notes".to_owned(),
    ///     Some(StorePath::new("vault")?),
    /// );
    /// assert_eq!(config.region, "eu-west-1");
    /// assert_eq!(config.prefix, Some(StorePath::new("vault")?));
    /// # Ok::<(), arky_store::StoreError>(())
    /// ```
    #[must_use]
    pub const fn new(region: String, bucket: String, prefix: Option<StorePath>) -> Self {
        Self {
            bucket,
            prefix,
            region,
        }
    }
}

/// The bucket's condition for `precondition`, a revision being the entity tag.
fn condition(precondition: &Precondition) -> Condition {
    match precondition {
        Precondition::Absent => Condition::Absent,
        Precondition::Matches(revision) => Condition::ETag(revision.0.clone()),
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

/// `NotFound` or `Conflict` of `path` when `error` says so, else `Backend`
/// naming `path`.
fn failure(path: &StorePath, error: &ObjectError) -> StoreError {
    match error.kind() {
        ObjectErrorKind::NotFound => StoreError::NotFound(path.clone()),
        ObjectErrorKind::PreconditionFailed => StoreError::Conflict(path.clone()),
        ObjectErrorKind::Other | _ => StoreError::Backend(format!("{path}: {error}")),
    }
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

#[cfg(test)]
mod tests;
