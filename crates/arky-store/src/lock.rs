//! The lock a save holds on a document in the file store and the bucket
//! stores.
//!
//! A save through [`Store::write_with_embedded`] takes the lock of the
//! document, `<path>.arky-lock` beside it, and checks the precondition under
//! it. Before each file, and before the document, it checks that it still
//! holds the lock and that the lock has not expired. After the document it
//! gives the lock back, only while the lock still holds this save's GUID. In
//! the file store a lock is a file holding the save's GUID, and its age counts
//! from the file's modification time. In a bucket it is an object holding
//! `<guid> <seconds since 1970-01-01 UTC>`; one whose text does not read
//! counts as expired.
//!
//! A save that finds a lock that has not expired waits [`LockSettings::wait`]
//! once and looks again; a lock still there and not expired fails the save
//! with [`StoreError::Conflict`], before it writes anything. A lock older than
//! [`LockSettings::expires`] has expired: a save removes it and takes its own,
//! and `remove_expired_locks` on the file store or a bucket store removes
//! every one. A save that runs past `expires` fails with `Conflict` before its
//! next write. A plain [`Store::write`] or [`Store::delete`] takes no lock,
//! and the `MongoDB` store takes none at all.
//!
//! A store outside this crate implements [`Locks`] and calls
//! [`write_with_embedded`] from its own `Store::write_with_embedded`, as the
//! file store and the bucket stores do.

use alloc::collections::BTreeMap;
use core::time::Duration;
use std::time::SystemTime;

use os_shim::System;
use uuid::Uuid;

use crate::{Precondition, Revision, Store, StoreError, StorePath, write_at_paths};

/// The end of a lock's name: the lock of `notes/report.md` is
/// `notes/report.md.arky-lock`.
pub const SUFFIX: &str = ".arky-lock";

/// How many passes `take` makes before it gives up.
const TAKE_TRIES: u32 = 10;

/// How long a lock lasts after it is taken, and how long a save waits on a
/// lock of another save that has not expired.
///
/// A store takes them through its `locking` method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LockSettings {
    /// How long a lock lasts after it is taken. An older lock has expired.
    pub expires: Duration,
    /// How long a save waits, once, on a lock of another save that has not
    /// expired.
    pub wait: Duration,
}

impl LockSettings {
    /// Settings whose locks last `expires` and whose saves wait `wait`.
    ///
    /// ```rust
    /// use std::time::Duration;
    ///
    /// use arky_store::lock::LockSettings;
    ///
    /// let slow_saves = LockSettings::new(Duration::from_secs(60), Duration::from_secs(10));
    /// assert_eq!(slow_saves.expires, Duration::from_secs(60));
    /// assert_eq!(slow_saves.wait, Duration::from_secs(10));
    ///
    /// let usual = LockSettings::new(Duration::from_secs(5), Duration::from_secs(2));
    /// assert_eq!(LockSettings::default(), usual);
    /// ```
    #[must_use]
    pub const fn new(expires: Duration, wait: Duration) -> Self {
        Self { expires, wait }
    }
}

impl Default for LockSettings {
    /// A lock lasts 5 seconds, and a save waits 2.
    fn default() -> Self {
        Self::new(Duration::from_secs(5), Duration::from_secs(2))
    }
}

/// A lock as a store finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Found {
    /// The GUID of the save that took it; empty when the lock does not read.
    pub holder: String,
    /// What a remove compares: the file's bytes, or the object's `ETag`.
    pub tag: Vec<u8>,
    /// When it was taken; the Unix epoch when the lock does not read, so it
    /// counts as expired.
    pub taken: SystemTime,
}

impl Found {
    /// The lock `holder` took at `taken`, which a remove compares by `tag`.
    #[must_use]
    pub const fn new(holder: String, tag: Vec<u8>, taken: SystemTime) -> Self {
        Self { holder, tag, taken }
    }
}

/// What a store does with a lock: `create`, `find`, `remove_if`, `revision`
/// and `system`. A store implements them to hold the lock of a save through
/// [`write_with_embedded`].
pub trait Locks {
    /// Makes the lock at `lock` holding `guid` only when nothing is there:
    /// `false` when something is.
    fn create(
        &self,
        lock: &StorePath,
        guid: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The lock at `lock`, or `None` where nothing is.
    fn find(
        &self,
        lock: &StorePath,
    ) -> impl Future<Output = Result<Option<Found>, StoreError>> + Send;

    /// Removes the lock at `lock` only while it is still `found`: `false` when
    /// it is not, or is gone.
    fn remove_if(
        &self,
        lock: &StorePath,
        found: &Found,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The revision of the file at `path`, or `None` where nothing is.
    fn revision(
        &self,
        path: &StorePath,
    ) -> impl Future<Output = Result<Option<Revision>, StoreError>> + Send;

    /// The clock: `now` and `sleep_async`.
    fn system(&self) -> &dyn System;
}

/// A lock a save holds.
pub(crate) struct Held {
    pub guid: String,
    pub lock: StorePath,
}

/// `Ok` while `held` is the lock at its path and has not expired;
/// `StoreError::Conflict` of `path` otherwise.
pub(crate) async fn check<L>(
    store: &L,
    held: &Held,
    path: &StorePath,
    settings: LockSettings,
) -> Result<(), StoreError>
where
    L: Locks + Sync,
{
    let now = store.system().now();
    match store.find(&held.lock).await? {
        Some(found) if found.holder == held.guid && !expired(&found, now, settings) => Ok(()),
        Some(_) | None => Err(StoreError::Conflict(path.clone())),
    }
}

/// Whether `found` has expired at `now`.
pub(crate) fn expired(found: &Found, now: SystemTime, settings: LockSettings) -> bool {
    now.duration_since(found.taken)
        .is_ok_and(|age| age > settings.expires)
}

/// Removes each lock of `locks` that has expired, each only while it is still
/// the lock found, and gives the paths of those it removed.
///
/// # Errors
///
/// The errors of `store`'s `find` and `remove_if`.
pub async fn remove_expired<L>(
    store: &L,
    locks: &[StorePath],
    settings: LockSettings,
) -> Result<Vec<StorePath>, StoreError>
where
    L: Locks + Sync,
{
    let mut removed = Vec::new();
    for lock in locks {
        let Some(found) = store.find(lock).await? else {
            continue;
        };
        if expired(&found, store.system().now(), settings) && store.remove_if(lock, &found).await? {
            removed.push(lock.clone());
        }
    }
    Ok(removed)
}

/// Gives back `held` once the save is done: removes it only while it still
/// holds this save's GUID, and leaves it otherwise. A failure is folded into
/// the save's result as a `StoreError::Backend` that says whether the
/// document is stored.
pub(crate) async fn finish<L>(
    store: &L,
    held: &Held,
    path: &StorePath,
    saved: Result<Revision, StoreError>,
) -> Result<Revision, StoreError>
where
    L: Locks + Sync,
{
    match (give_back(store, held).await, saved) {
        (Ok(()), result) => result,
        (Err(error), Ok(_)) => Err(StoreError::Backend(format!(
            "{path}: the document is stored, and its lock was not given back: {error}; it expires by itself"
        ))),
        (Err(error), Err(failed)) => Err(StoreError::Backend(format!(
            "{failed}; its lock was not given back: {error}"
        ))),
    }
}

/// Removes `held` while it is still the lock at its path with its GUID.
async fn give_back<L>(store: &L, held: &Held) -> Result<(), StoreError>
where
    L: Locks + Sync,
{
    if let Some(found) = store.find(&held.lock).await?
        && found.holder == held.guid
    {
        // `false` is a lock another save took in the meantime, which stays.
        let _: bool = store.remove_if(&held.lock, &found).await?;
    }
    Ok(())
}

/// The path of the lock of the file at `path`.
///
/// ```rust
/// use arky_store::StorePath;
/// use arky_store::lock::lock_of;
///
/// let report = StorePath::new("notes/report.md")?;
/// assert_eq!(lock_of(&report)?.as_str(), "notes/report.md.arky-lock");
/// # Ok::<(), arky_store::StoreError>(())
/// ```
///
/// # Errors
///
/// `StoreError::InvalidPath` as `StorePath::new` gives it.
pub fn lock_of(path: &StorePath) -> Result<StorePath, StoreError> {
    StorePath::new(&format!("{path}{SUFFIX}"))
}

/// Takes the lock of `path` for one save: up to `TAKE_TRIES` passes of
/// create; else find; an expired lock is removed only while it is still the
/// one found, and the pass starts over; a lock that has not expired is waited
/// on once and looked at again; a second lock that has not expired, or the
/// passes running out, is `StoreError::Conflict` of `path`.
pub(crate) async fn take<L>(
    store: &L,
    path: &StorePath,
    settings: LockSettings,
) -> Result<Held, StoreError>
where
    L: Locks + Sync,
{
    let held = Held {
        guid: Uuid::new_v4().to_string(),
        lock: lock_of(path)?,
    };
    let mut waited = false;
    for _ in 0..TAKE_TRIES {
        if store.create(&held.lock, &held.guid).await? {
            return Ok(held);
        }
        let Some(found) = store.find(&held.lock).await? else {
            continue;
        };
        if expired(&found, store.system().now(), settings) {
            let _: bool = store.remove_if(&held.lock, &found).await?;
        } else if waited {
            break;
        } else {
            store
                .system()
                .sleep_async(settings.wait)
                .await
                .map_err(|error| StoreError::Backend(format!("{path}: {error}")))?;
            waited = true;
        }
    }
    Err(StoreError::Conflict(path.clone()))
}

/// What `Store::write_with_embedded` does in the file store and the bucket
/// stores, under the lock of `path`.
///
/// It takes the lock, tests `precondition` under it, writes each file of
/// `files` at its path, then `text` at `path` under `precondition`, checks the
/// lock before each write, and gives it back after the document.
///
/// # Errors
///
/// `StoreError::Conflict` when `precondition` does not hold or another save
/// holds the lock, and the errors of `store`'s writes.
pub async fn write_with_embedded<S>(
    store: &S,
    path: &StorePath,
    text: &str,
    files: &BTreeMap<StorePath, Vec<u8>>,
    precondition: &Precondition,
    settings: LockSettings,
) -> Result<Revision, StoreError>
where
    S: Store + Locks + Sync,
{
    let held = take(store, path, settings).await?;
    let saved = match holds(store, path, precondition).await {
        Ok(()) => {
            write_at_paths(store, path, text, files, precondition, || {
                check(store, &held, path, settings)
            })
            .await
        }
        Err(error) => Err(error),
    };
    finish(store, &held, path, saved).await
}

/// `Ok` when `precondition` holds for the document at `path` as it is now;
/// `StoreError::Conflict` of `path` otherwise.
async fn holds<S>(
    store: &S,
    path: &StorePath,
    precondition: &Precondition,
) -> Result<(), StoreError>
where
    S: Locks + Sync,
{
    if matches!(precondition, Precondition::None) {
        return Ok(());
    }
    let current = store.revision(path).await?;
    let holds = match precondition {
        Precondition::Absent => current.is_none(),
        Precondition::Matches(expected) => current.as_ref() == Some(expected),
        Precondition::None => true,
    };
    if holds {
        Ok(())
    } else {
        Err(StoreError::Conflict(path.clone()))
    }
}

#[cfg(test)]
mod tests;
