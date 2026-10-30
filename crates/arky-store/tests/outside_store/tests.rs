use alloc::collections::BTreeMap;
use core::future;
use core::str;
use core::time::Duration;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use arky::bundle::{BundledDocument, Missing};
use arky::forms::Form;
use arky::parser::parse;
use arky::writer::{WriteOptions, write};
use arky_store::document::{self, DocumentError};
use arky_store::lock::{self, Found, LockSettings, Locks};
use arky_store::{Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored};
use os_shim::System;
use os_shim::mock::MemorySystem;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

/// Each file's bytes and how many times it was written, keyed by its path.
type Files = BTreeMap<String, (Vec<u8>, u64)>;

/// Files in a map, each one's revision the count of its writes. A lock is a
/// file holding `<guid> <seconds since 1970-01-01 UTC>`.
#[derive(Default)]
struct MapStore {
    files: Mutex<Files>,
    system: MemorySystem,
}

impl MapStore {
    /// Deletes the file at `path` when `precondition` holds.
    fn deleted(&self, path: &StorePath, precondition: &Precondition) -> Result<(), StoreError> {
        let mut files = self.files();
        if !holds(version(&files, path), precondition) {
            return Err(StoreError::Conflict(path.clone()));
        }
        files
            .remove(path.as_str())
            .map(|_| ())
            .ok_or_else(|| StoreError::NotFound(path.clone()))
    }

    fn files(&self) -> MutexGuard<'_, Files> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The lock at `lock`; one whose bytes do not read was taken at the Unix
    /// epoch, so it has expired.
    fn found(&self, lock: &StorePath) -> Option<Found> {
        let tag = self.files().get(lock.as_str())?.0.clone();
        let (holder, taken) = held_by(&tag).unwrap_or((String::new(), SystemTime::UNIX_EPOCH));
        Some(Found::new(holder, tag, taken))
    }

    /// The path of each file under `prefix`, locks included, sorted.
    fn keys(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let under = prefix.map(|folder| format!("{folder}/"));
        self.files()
            .keys()
            .filter(|key| under.as_ref().is_none_or(|folder| key.starts_with(folder)))
            .map(|key| StorePath::new(key))
            .collect()
    }

    /// Makes the lock at `lock` holding `guid` and the time now, only when
    /// nothing is there.
    fn made(&self, lock: &StorePath, guid: &str) -> Result<bool, StoreError> {
        let taken = self
            .system
            .now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|error| StoreError::Backend(format!("{lock}: {error}")))?;
        let bytes = format!("{guid} {}", taken.as_secs()).into_bytes();
        let mut files = self.files();
        let free = !files.contains_key(lock.as_str());
        if free {
            files.insert(lock.as_str().to_owned(), (bytes, 1));
        }
        drop(files);
        Ok(free)
    }

    /// Removes the lock at `lock` only while it holds the bytes of `found`.
    fn removed(&self, lock: &StorePath, found: &Found) -> bool {
        let mut files = self.files();
        let still = files
            .get(lock.as_str())
            .is_some_and(|(bytes, _)| *bytes == found.tag);
        if still {
            files.remove(lock.as_str());
        }
        still
    }

    fn stored(&self, path: &StorePath) -> Result<Stored, StoreError> {
        self.files()
            .get(path.as_str())
            .map(|(bytes, written)| Stored::new(bytes.clone(), revision(*written)))
            .ok_or_else(|| StoreError::NotFound(path.clone()))
    }

    /// Writes `bytes` at `path` when `precondition` holds, checked and written
    /// under one hold of the map.
    fn written(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        let mut files = self.files();
        let current = version(&files, path);
        if !holds(current, precondition) {
            return Err(StoreError::Conflict(path.clone()));
        }
        let next = current.map_or(1, |written| written + 1);
        files.insert(path.as_str().to_owned(), (bytes.to_vec(), next));
        drop(files);
        Ok(revision(next))
    }
}

impl Locks for MapStore {
    fn create(
        &self,
        lock: &StorePath,
        guid: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        future::ready(self.made(lock, guid))
    }

    fn find(
        &self,
        lock: &StorePath,
    ) -> impl Future<Output = Result<Option<Found>, StoreError>> + Send {
        future::ready(Ok(self.found(lock)))
    }

    fn remove_if(
        &self,
        lock: &StorePath,
        found: &Found,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        future::ready(Ok(self.removed(lock, found)))
    }

    fn revision(
        &self,
        path: &StorePath,
    ) -> impl Future<Output = Result<Option<Revision>, StoreError>> + Send {
        future::ready(Ok(version(&self.files(), path).map(revision)))
    }

    fn system(&self) -> &dyn System {
        &self.system
    }
}

impl Store for MapStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities::new(true)
    }

    fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        future::ready(self.deleted(path, precondition))
    }

    fn ensure_container(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
        future::ready(Err(StoreError::Unsupported("create its container")))
    }

    fn finish_deletions(&self) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        future::ready(Err(StoreError::Unsupported("finish deletions")))
    }

    fn list(
        &self,
        prefix: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        future::ready(self.keys(prefix).map(|mut paths| {
            paths.retain(|path| !path.as_str().ends_with(lock::SUFFIX));
            paths
        }))
    }

    fn read(&self, path: &StorePath) -> impl Future<Output = Result<Stored, StoreError>> + Send {
        future::ready(self.stored(path))
    }

    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send {
        future::ready(self.stored(file).map(|stored| stored.bytes))
    }

    async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        let mut locks = self.keys(under)?;
        locks.retain(|path| path.as_str().ends_with(lock::SUFFIX));
        lock::remove_expired(self, &locks, LockSettings::default()).await
    }

    fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        future::ready(self.written(path, bytes, precondition))
    }

    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        lock::write_with_embedded(
            self,
            path,
            text,
            files,
            precondition,
            LockSettings::default(),
        )
    }
}

/// The GUID and the time a lock holds, or `None` when its bytes do not read
/// as `<guid> <seconds>`.
fn held_by(bytes: &[u8]) -> Option<(String, SystemTime)> {
    let (guid, seconds) = str::from_utf8(bytes).ok()?.split_once(' ')?;
    let taken = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds.parse().ok()?))?;
    Some((guid.to_owned(), taken))
}

/// Whether `precondition` holds for a file written `current` times, `None`
/// where no file is.
fn holds(current: Option<u64>, precondition: &Precondition) -> bool {
    match precondition {
        Precondition::Absent => current.is_none(),
        Precondition::Matches(expected) => current.map(revision).as_ref() == Some(expected),
        Precondition::None | _ => true,
    }
}

fn path(text: &str) -> StorePath {
    StorePath::new(text).unwrap()
}

fn revision(written: u64) -> Revision {
    Revision::new(written.to_string())
}

/// A document embedding `media/logo.png`, with that picture.
fn report() -> BundledDocument {
    let text = "# Report\n\n![Logo](media/logo.png)\n";
    let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
    BundledDocument::new(parse(text).document, files, Missing::Refuse).unwrap()
}

/// How many times the file at `path` was written, or `None` where no file is.
fn version(files: &Files, path: &StorePath) -> Option<u64> {
    files.get(path.as_str()).map(|(_, written)| *written)
}

#[tokio::test]
async fn a_put_then_a_get_gives_the_document_back_in_arkys_form() {
    let store = MapStore::default();
    let note = path("notes/a.md");
    let options = WriteOptions::default();
    let first = document::put(
        &store,
        &note,
        Form::Markdown,
        b"# A\n\n* item\n",
        &options,
        &Precondition::Absent,
        Missing::Refuse,
    )
    .await
    .unwrap();
    assert_eq!(first, Revision::new("1"));
    assert_eq!(store.read(&note).await.unwrap().bytes, b"# A\n\n- item\n");
    let back = document::get(&store, &note, Missing::Refuse).await.unwrap();
    assert_eq!(back.files(), Vec::<&str>::new());
    assert_eq!(write(back.document(), &options).unwrap(), "# A\n\n- item\n");
    assert_eq!(store.list(None).await.unwrap(), [note]);
}

#[tokio::test]
async fn a_second_put_under_absent_is_a_conflict_naming_the_path() {
    let store = MapStore::default();
    let note = path("notes/a.md");
    let options = WriteOptions::default();
    let put = || {
        document::put(
            &store,
            &note,
            Form::Markdown,
            b"# A\n",
            &options,
            &Precondition::Absent,
            Missing::Refuse,
        )
    };
    put().await.unwrap();
    let error = put().await.unwrap_err();
    assert!(
        matches!(&error, DocumentError::Store(StoreError::Conflict(at)) if *at == note),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "notes/a.md: the file changed, or a file is already there"
    );
    assert_eq!(store.read(&note).await.unwrap().bytes, b"# A\n");
}

#[tokio::test]
async fn a_save_with_an_embedded_file_leaves_no_lock_behind() {
    let store = MapStore::default();
    let note = path("notes/report.md");
    let bundle = report();
    document::put_bundle(
        &store,
        &note,
        &bundle,
        &WriteOptions::default(),
        &Precondition::Absent,
    )
    .await
    .unwrap();
    let keys: Vec<String> = store.files().keys().cloned().collect();
    assert_eq!(keys, ["notes/media/logo.png", "notes/report.md"]);
    let back = document::get(&store, &note, Missing::Refuse).await.unwrap();
    assert_eq!(back, bundle);
}

#[tokio::test]
async fn a_save_while_another_saves_lock_has_not_expired_is_a_conflict_before_any_write() {
    let store = MapStore::default();
    let note = path("notes/report.md");
    let held = lock::lock_of(&note).unwrap();
    assert!(store.create(&held, "another-save").await.unwrap());
    let saved = document::put_bundle(
        &store,
        &note,
        &report(),
        &WriteOptions::default(),
        &Precondition::None,
    )
    .await;
    assert!(
        matches!(&saved, Err(DocumentError::Store(StoreError::Conflict(at))) if *at == note),
        "{saved:?}"
    );
    let keys: Vec<String> = store.files().keys().cloned().collect();
    assert_eq!(keys, ["notes/report.md.arky-lock"]);
    let found = store.find(&held).await.unwrap().unwrap();
    assert_eq!(found.holder, "another-save");
}

#[tokio::test]
async fn remove_expired_locks_removes_only_a_lock_that_has_expired() {
    let store = MapStore::default();
    let stopped = lock::lock_of(&path("notes/stopped.md")).unwrap();
    assert!(store.create(&stopped, "stopped-save").await.unwrap());
    store.system.advance(Duration::from_secs(6)).unwrap();
    let running = lock::lock_of(&path("notes/running.md")).unwrap();
    assert!(store.create(&running, "running-save").await.unwrap());
    assert_eq!(store.remove_expired_locks(None).await.unwrap(), [stopped]);
    let keys: Vec<String> = store.files().keys().cloned().collect();
    assert_eq!(keys, ["notes/running.md.arky-lock"]);
}
