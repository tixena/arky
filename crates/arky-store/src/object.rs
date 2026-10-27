//! Files kept as objects in one bucket, through os-shim's `Bucket`.

use alloc::collections::BTreeMap;
use core::fmt;
use core::str;
use core::time::Duration;
use std::env::VarError;
use std::time::SystemTime;

use os_shim::System;
use os_shim::bucket::{Bucket, Condition, MinioBucket, MinioConfig, ObjectError, ObjectErrorKind};

use crate::lock::{self, Found, LockSettings, Locks};
use crate::{
    Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored, read_at_path,
};

/// The longest key S3 takes, in bytes.
const LONGEST_KEY: usize = 1024;

/// Markdown files in one bucket, under an optional key prefix. Over
/// `MinioBucket`, a delete's precondition is checked by a request of its own
/// before the delete, not in one step with it.
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
    #[must_use]
    pub fn locking(self, lock: LockSettings) -> Self {
        Self { lock, ..self }
    }

    /// A store over `bucket`, its keys under `prefix`, its clock `system`'s.
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
    /// # Errors
    ///
    /// `StoreError::Backend` when the store cannot be listed or a lock cannot
    /// be read or removed.
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

impl<'system> ObjectStore<'system, MinioBucket> {
    /// A store over the `MinIO` server `config` names, its keys read from the
    /// environment.
    ///
    /// # Errors
    ///
    /// `Backend` naming the variable and what it holds when `AWS_ACCESS_KEY_ID`
    /// or `AWS_SECRET_ACCESS_KEY` is not set or is not UTF-8.
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObjectConfig {
    pub bucket: String,
    /// The server's address, such as `http://127.0.0.1:9000`; required, no
    /// default.
    pub endpoint: String,
    pub prefix: Option<StorePath>,
    pub region: String,
}

impl ObjectConfig {
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
