//! The store over `MemoryBucket`, and over a bucket whose every call fails;
//! under `cfg(store_tests)`, over `MinioBucket` on the `RustFS` server
//! `just test-stores` starts.

#[cfg(store_tests)]
mod server;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::future;
use std::io;
use std::sync::Mutex;

use arky::bundle::{BundledDocument, Missing};
use arky::forms::{Form, encode};
use arky::parser::parse;
use arky::writer::WriteOptions;
use core::time::Duration;
use std::time::SystemTime;

use os_shim::bucket::{Bucket, Condition, Head, MemoryBucket, Object, ObjectError};
use os_shim::mock::MemorySystem;

use super::{ObjectConfig, ObjectStore};
use crate::any::AnyStore;
use crate::document::{get, put};
use crate::lock::{Locks as _, lock_of};
use crate::{Precondition, Revision, Store as _, StoreError, StorePath};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The secret key `open` reads in these tests.
const SECRET: &str = "arky-unit-secret";

/// A bucket whose every call fails as an unreachable server's would.
struct DownBucket;

impl Bucket for DownBucket {
    fn delete(
        &self,
        key: &str,
        _condition: &Condition,
    ) -> impl Future<Output = Result<(), ObjectError>> + Send {
        future::ready(Err(down(key)))
    }

    fn get(&self, key: &str) -> impl Future<Output = Result<Object, ObjectError>> + Send {
        future::ready(Err(down(key)))
    }

    fn head(&self, key: &str) -> impl Future<Output = Result<Head, ObjectError>> + Send {
        future::ready(Err(down(key)))
    }

    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>, ObjectError>> + Send {
        future::ready(Err(down(prefix)))
    }

    fn put(
        &self,
        key: &str,
        _bytes: &[u8],
        _condition: &Condition,
    ) -> impl Future<Output = Result<String, ObjectError>> + Send {
        future::ready(Err(down(key)))
    }
}

/// A `MemoryBucket` that records each `get` and `head` it answers, with its key.
#[derive(Clone, Default)]
struct Watched {
    bucket: MemoryBucket,
    reads: Arc<Mutex<Vec<String>>>,
}

impl Watched {
    fn reads(&self) -> Vec<String> {
        self.reads.lock().unwrap().clone()
    }
}

impl Bucket for Watched {
    fn delete(
        &self,
        key: &str,
        condition: &Condition,
    ) -> impl Future<Output = Result<(), ObjectError>> + Send {
        self.bucket.delete(key, condition)
    }

    fn get(&self, key: &str) -> impl Future<Output = Result<Object, ObjectError>> + Send {
        self.reads.lock().unwrap().push(format!("get {key}"));
        self.bucket.get(key)
    }

    fn head(&self, key: &str) -> impl Future<Output = Result<Head, ObjectError>> + Send {
        self.reads.lock().unwrap().push(format!("head {key}"));
        self.bucket.head(key)
    }

    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>, ObjectError>> + Send {
        self.bucket.list(prefix)
    }

    fn put(
        &self,
        key: &str,
        bytes: &[u8],
        condition: &Condition,
    ) -> impl Future<Output = Result<String, ObjectError>> + Send {
        self.bucket.put(key, bytes, condition)
    }
}

/// An `Other` failure naming `key`.
fn down(key: &str) -> ObjectError {
    ObjectError::from(io::Error::other(format!("no answer for `{key}`")))
}

/// A store over a bucket `memory` shares, its keys under `vault`.
fn in_vault<'system>(
    memory: &MemoryBucket,
    system: &'system MemorySystem,
) -> ObjectStore<'system, MemoryBucket> {
    ObjectStore::new(memory.clone(), Some(path("vault")), system)
}

fn names(paths: &[StorePath]) -> Vec<&str> {
    paths.iter().map(StorePath::as_str).collect()
}

fn path(text: &str) -> StorePath {
    StorePath::new(text).unwrap()
}

/// The configuration of a server no test reaches.
fn nowhere() -> ObjectConfig {
    ObjectConfig::new(
        "http://127.0.0.1:9".to_owned(),
        "us-east-1".to_owned(),
        "arky".to_owned(),
        Some(path("vault")),
    )
}

/// A store path of `length` bytes.
fn of_length(length: usize) -> StorePath {
    path(&format!("notes/{}.md", "a".repeat(length - 9)))
}

#[tokio::test]
async fn the_sample_writes_reads_refuses_a_second_file_at_a_path_and_lists() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(MemoryBucket::default(), Some(path("vault")), &system);
    let task = path("notes/task.md");
    let revision = store
        .write(&task, b"# Task\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(store.read(&task).await.unwrap().revision, revision);
    assert!(matches!(
        store
            .write(&task, b"# Again\n", &Precondition::Absent)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.list(None).await.unwrap(), vec![task]);
}

#[tokio::test]
async fn a_file_is_the_object_at_its_path_under_the_prefix_or_at_its_path_with_none() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let task = path("notes/task.md");
    in_vault(&memory, &system)
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    ObjectStore::new(memory.clone(), None, &system)
        .write(&task, b"# Root\n", &Precondition::None)
        .await
        .unwrap();
    let keys = memory.list("").await.unwrap();
    assert_eq!(keys, ["notes/task.md", "vault/notes/task.md"]);
    let object = memory.get("vault/notes/task.md").await.unwrap();
    assert_eq!(object.bytes, b"# Task\n");
}

#[tokio::test]
async fn a_read_gives_the_bytes_and_the_tag_unchanged_and_no_object_is_not_found() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let task = path("notes/task.md");
    let tag = memory
        .put("vault/notes/task.md", b"# Task\n", &Condition::None)
        .await
        .unwrap();
    assert!(tag.starts_with('"') && tag.ends_with('"'), "{tag}");
    let read = store.read(&task).await.unwrap();
    assert_eq!(read.bytes, b"# Task\n");
    assert_eq!(read.revision, Revision(tag));
    let other = path("notes/other.md");
    assert_eq!(store.read(&other).await, Err(StoreError::NotFound(other)));
}

#[tokio::test]
async fn a_write_with_no_precondition_replaces_the_object_and_gives_its_tag() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let task = path("notes/task.md");
    let first = store
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    let second = store
        .write(&task, b"# Done\n", &Precondition::None)
        .await
        .unwrap();
    assert_ne!(first, second);
    let object = memory.get("vault/notes/task.md").await.unwrap();
    assert_eq!(object.bytes, b"# Done\n");
    assert_eq!(Revision(object.etag), second);
}

#[tokio::test]
async fn a_precondition_that_does_not_hold_is_a_conflict_and_changes_nothing() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(MemoryBucket::default(), Some(path("vault")), &system);
    let task = path("notes/task.md");
    let first = store
        .write(&task, b"# Task\n", &Precondition::Absent)
        .await
        .unwrap();
    let second = store
        .write(&task, b"# Done\n", &Precondition::Matches(first.clone()))
        .await
        .unwrap();
    let kept = store.read(&task).await.unwrap();
    assert_eq!(kept.revision, second);
    let conflict = StoreError::Conflict(task.clone());
    let absent = store.write(&task, b"# B\n", &Precondition::Absent).await;
    assert_eq!(absent, Err(conflict.clone()));
    let stale = Precondition::Matches(first);
    let late = store.write(&task, b"# B\n", &stale).await;
    assert_eq!(late, Err(conflict.clone()));
    assert_eq!(store.delete(&task, &stale).await, Err(conflict.clone()));
    let held = store.delete(&task, &Precondition::Absent).await;
    assert_eq!(held, Err(conflict));
    assert_eq!(store.read(&task).await.unwrap(), kept);
    assert_eq!(names(&store.list(None).await.unwrap()), ["notes/task.md"]);

    let other = path("notes/other.md");
    let unmatched = store
        .write(&other, b"# C\n", &Precondition::Matches(second))
        .await;
    assert_eq!(unmatched, Err(StoreError::Conflict(other)));
}

#[tokio::test]
async fn a_delete_removes_the_object_and_one_where_no_file_is_succeeds_without_a_revision() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let task = path("notes/task.md");
    let revision = store
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    store
        .delete(&task, &Precondition::Matches(revision.clone()))
        .await
        .unwrap();
    assert_eq!(memory.list("").await.unwrap(), Vec::<String>::new());
    store
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    store.delete(&task, &Precondition::None).await.unwrap();
    assert_eq!(memory.list("").await.unwrap(), Vec::<String>::new());

    store.delete(&task, &Precondition::None).await.unwrap();
    store.delete(&task, &Precondition::Absent).await.unwrap();
    let gone = store.delete(&task, &Precondition::Matches(revision)).await;
    assert_eq!(gone, Err(StoreError::Conflict(task)));
}

#[tokio::test]
async fn a_listing_takes_the_prefix_off_leaves_folders_out_and_sorts_by_bytes() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let keys = [
        "vault/notes/b.md",
        "vault/notesx/a.md",
        "vault/notes/",
        "vault/notes/a/c.md",
        "vault/Z.md",
        "vault/no.es/d.md",
        "other/notes/e.md",
        "vaultx/f.md",
    ];
    for key in keys {
        memory
            .put(key, b"# File\n", &Condition::None)
            .await
            .unwrap();
    }
    let store = in_vault(&memory, &system);
    let all = store.list(None).await.unwrap();
    assert_eq!(
        names(&all),
        [
            "Z.md",
            "no.es/d.md",
            "notes/a/c.md",
            "notes/b.md",
            "notesx/a.md"
        ]
    );
    let notes = store.list(Some(&path("notes"))).await.unwrap();
    assert_eq!(names(&notes), ["notes/a/c.md", "notes/b.md"]);

    let root = ObjectStore::new(memory.clone(), None, &system);
    let in_other = root.list(Some(&path("other"))).await.unwrap();
    assert_eq!(names(&in_other), ["other/notes/e.md"]);
    assert_eq!(root.list(None).await.unwrap().len(), keys.len() - 1);
}

#[tokio::test]
async fn a_listed_key_that_is_not_a_store_path_is_an_invalid_path_naming_it() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    memory
        .put("vault/a//b", b"# File\n", &Condition::None)
        .await
        .unwrap();
    let listed = in_vault(&memory, &system).list(None).await;
    assert_eq!(listed, Err(StoreError::InvalidPath("a//b".to_owned())));
}

#[tokio::test]
async fn a_key_past_1024_bytes_fails_the_write_with_the_path_before_any_request() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let longest = of_length(1018);
    let past = of_length(1019);
    in_vault(&memory, &system)
        .write(&longest, b"# File\n", &Precondition::None)
        .await
        .unwrap();
    let refused = Err(StoreError::InvalidPath(past.as_str().to_owned()));
    let written = in_vault(&memory, &system)
        .write(&past, b"# File\n", &Precondition::None)
        .await;
    assert_eq!(written, refused);
    let down = ObjectStore::new(DownBucket, Some(path("vault")), &system);
    let unsent = down.write(&past, b"# File\n", &Precondition::Absent).await;
    assert_eq!(unsent, refused);
    let keys = memory.list("").await.unwrap();
    assert_eq!(keys, [format!("vault/{longest}")]);

    let root = ObjectStore::new(memory.clone(), None, &system);
    root.write(&past, b"# File\n", &Precondition::None)
        .await
        .unwrap();
}

#[tokio::test]
async fn each_failure_of_the_bucket_is_a_backend_error_naming_the_path() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(DownBucket, Some(path("vault")), &system);
    let task = path("notes/task.md");
    let failed = "notes/task.md: no answer for `vault/notes/task.md`";
    let backend = StoreError::Backend(failed.to_owned());
    assert_eq!(store.read(&task).await, Err(backend.clone()));
    for precondition in [Precondition::None, Precondition::Absent] {
        let written = store.write(&task, b"# Task\n", &precondition).await;
        assert_eq!(written, Err(backend.clone()));
        let deleted = store.delete(&task, &precondition).await;
        assert_eq!(deleted, Err(backend.clone()));
    }
    let notes = store.list(Some(&path("notes"))).await;
    let in_notes = "notes: no answer for `vault/notes/`".to_owned();
    assert_eq!(notes, Err(StoreError::Backend(in_notes)));
    let all = store.list(None).await;
    assert_eq!(
        all,
        Err(StoreError::Backend("no answer for `vault/`".to_owned()))
    );
    let root = ObjectStore::new(DownBucket, None, &system).list(None).await;
    assert_eq!(
        root,
        Err(StoreError::Backend("no answer for ``".to_owned()))
    );
}

#[tokio::test]
async fn a_put_keeps_each_embedded_file_as_an_object_beside_the_document_that_a_get_gives_back() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let options = WriteOptions::default();
    let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
    let bundle = BundledDocument::new(parse(REPORT).document, files, Missing::Refuse).unwrap();
    let zip = encode(&bundle, Form::Zip, &options).unwrap();
    let report = path("notes/report.md");
    put(
        &store,
        &report,
        Form::Zip,
        &zip,
        &options,
        &Precondition::None,
        Missing::Refuse,
    )
    .await
    .unwrap();
    let keys = memory.list("").await.unwrap();
    assert_eq!(
        keys,
        ["vault/notes/media/logo.png", "vault/notes/report.md"]
    );
    let text = memory.get("vault/notes/report.md").await.unwrap().bytes;
    assert_eq!(text, REPORT.as_bytes());
    let logo = memory
        .get("vault/notes/media/logo.png")
        .await
        .unwrap()
        .bytes;
    assert_eq!(logo, PNG);
    assert_eq!(get(&store, &report, Missing::Refuse).await.unwrap(), bundle);
}

#[tokio::test]
async fn a_save_under_a_precondition_reads_the_documents_tag_and_not_its_bytes() {
    let system = MemorySystem::new();
    let watched = Watched::default();
    let store = ObjectStore::new(watched.clone(), Some(path("vault")), &system);
    let report = path("notes/report.md");
    let files = BTreeMap::new();
    let first = store
        .write_with_embedded(&report, REPORT, &files, &Precondition::Absent)
        .await
        .unwrap();
    let stale = Precondition::Matches(Revision("\"stale\"".to_owned()));
    let refused = store
        .write_with_embedded(&report, "# Stale\n", &files, &stale)
        .await;
    assert_eq!(refused, Err(StoreError::Conflict(report.clone())));
    store
        .write_with_embedded(&report, "# Next\n", &files, &Precondition::Matches(first))
        .await
        .unwrap();
    let of_report: Vec<String> = watched
        .reads()
        .into_iter()
        .filter(|read| read.ends_with("/report.md"))
        .collect();
    assert_eq!(of_report, ["head vault/notes/report.md"; 3]);
}

#[test]
fn a_precondition_is_checked_in_one_step_with_the_write() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(MemoryBucket::default(), None, &system);
    assert!(store.capabilities().atomic_preconditions);
}

#[test]
fn open_names_the_variable_that_is_not_set_and_never_shows_the_secret_key() {
    let none = MemorySystem::new();
    let access = StoreError::Backend(
        "AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key".to_owned(),
    );
    assert_eq!(ObjectStore::open(&none, nowhere()).err(), Some(access));
    let only_access = MemorySystem::new()
        .with_env("AWS_ACCESS_KEY_ID", "arky")
        .unwrap();
    let secret = StoreError::Backend(
        "AWS_SECRET_ACCESS_KEY is not set: it holds the MinIO secret key".to_owned(),
    );
    assert_eq!(
        ObjectStore::open(&only_access, nowhere()).err(),
        Some(secret)
    );
    let only_secret = MemorySystem::new()
        .with_env("AWS_SECRET_ACCESS_KEY", SECRET)
        .unwrap();
    let refused = ObjectStore::open(&only_secret, nowhere()).err().unwrap();
    assert!(!refused.to_string().contains(SECRET), "{refused}");
    assert!(!format!("{refused:?}").contains(SECRET), "{refused:?}");

    let both = MemorySystem::new()
        .with_env("AWS_ACCESS_KEY_ID", "arky")
        .unwrap()
        .with_env("AWS_SECRET_ACCESS_KEY", SECRET)
        .unwrap();
    let store = ObjectStore::open(&both, nowhere()).unwrap();
    let shown = format!("{store:?}");
    assert!(shown.contains("arky") && !shown.contains(SECRET), "{shown}");
    let any = AnyStore::Minio(store);
    assert!(any.capabilities().atomic_preconditions);
}

#[tokio::test]
async fn ensure_on_a_server_that_cannot_be_reached_is_a_backend_error_that_never_shows_the_secret_key()
 {
    let system = MemorySystem::new()
        .with_env("AWS_ACCESS_KEY_ID", "arky")
        .unwrap()
        .with_env("AWS_SECRET_ACCESS_KEY", SECRET)
        .unwrap();
    let store = ObjectStore::open(&system, nowhere()).unwrap();
    let refused = store.ensure().await.err().unwrap();
    assert!(matches!(refused, StoreError::Backend(_)), "{refused:?}");
    assert!(!refused.to_string().contains(SECRET), "{refused}");
}

#[tokio::test]
async fn a_lock_is_an_object_holding_the_guid_and_the_seconds_it_was_taken() {
    let taken = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000);
    let system = MemorySystem::new().with_now(taken).unwrap();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let lock = lock_of(&path("notes/report.md")).unwrap();
    assert!(store.create(&lock, "a-guid").await.unwrap());
    let object = memory.get("vault/notes/report.md.arky-lock").await.unwrap();
    assert_eq!(object.bytes, b"a-guid 1792700000");
    assert!(!store.create(&lock, "another").await.unwrap());
    let found = store.find(&lock).await.unwrap().unwrap();
    assert_eq!(found.holder, "a-guid");
    assert_eq!(found.taken, taken);
    assert_eq!(found.tag, object.etag.into_bytes());
}

#[tokio::test]
async fn a_lock_whose_bytes_do_not_read_counts_as_expired_and_the_save_removes_it() {
    let system = MemorySystem::new()
        .with_now(SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000))
        .unwrap();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    for bytes in [b"garbage".as_slice(), b"a-guid soon", b"\xff 12"] {
        memory
            .put("vault/notes/report.md.arky-lock", bytes, &Condition::None)
            .await
            .unwrap();
        let lock = lock_of(&path("notes/report.md")).unwrap();
        let found = store.find(&lock).await.unwrap().unwrap();
        assert_eq!(
            (found.holder.as_str(), found.taken),
            ("", SystemTime::UNIX_EPOCH)
        );
        let files = BTreeMap::from([(path("notes/media/logo.png"), PNG.to_vec())]);
        store
            .write_with_embedded(
                &path("notes/report.md"),
                REPORT,
                &files,
                &Precondition::None,
            )
            .await
            .unwrap();
        let keys = memory.list("").await.unwrap();
        assert_eq!(
            keys,
            ["vault/notes/media/logo.png", "vault/notes/report.md"]
        );
    }
}

#[tokio::test]
async fn a_save_whose_lock_key_is_past_1024_bytes_fails_with_the_documents_path_before_any_request()
{
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let files = BTreeMap::new();
    let longest = of_length(1008);
    in_vault(&memory, &system)
        .write_with_embedded(&longest, "# File\n", &files, &Precondition::None)
        .await
        .unwrap();
    let past = of_length(1009);
    let refused = Err(StoreError::InvalidPath(past.as_str().to_owned()));
    let written = in_vault(&memory, &system)
        .write_with_embedded(&past, "# File\n", &files, &Precondition::None)
        .await;
    assert_eq!(written, refused);
    let down = ObjectStore::new(DownBucket, Some(path("vault")), &system);
    let unsent = down
        .write_with_embedded(&past, "# File\n", &files, &Precondition::None)
        .await;
    assert_eq!(unsent, refused);
    assert_eq!(memory.list("").await.unwrap(), [format!("vault/{longest}")]);
}
