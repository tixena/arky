//! The store over `MemoryBucket`, and over a bucket whose every call fails;
//! under `cfg(store_tests)`, over `MinioBucket` on the `RustFS` server
//! `just test-stores` starts.

#[cfg(all(store_tests, feature = "minio"))]
mod server;

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arky::bundle::{BundledDocument, Missing};
use arky::forms::{Form, encode};
use arky::parser::parse;
use arky::writer::WriteOptions;
use core::time::Duration;
use std::time::SystemTime;

use os_shim::bucket::{Bucket as _, Condition, Depth, MemoryBucket, RevisionToken};
use os_shim::mock::MemorySystem;

#[cfg(feature = "minio")]
use super::ObjectConfig;
use super::ObjectStore;
use crate::any::AnyStore;
use crate::document::{get, put};
use crate::file::FileStore;
use crate::lock::{Locks as _, lock_of};
use crate::tests::TestBucket;
use crate::{Precondition, Revision, Store as _, StoreError, StorePath};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The secret key `open` reads in these tests.
const SECRET: &str = "arky-unit-secret";

/// The key of each object `memory` holds, sorted.
async fn keys(memory: &MemoryBucket) -> Vec<String> {
    let entries = memory.list(Path::new("/"), Depth::All).await.unwrap();
    entries
        .iter()
        .filter(|entry| entry.is_file)
        .map(|entry| entry.path.to_str().unwrap()[1..].to_owned())
        .collect()
}

/// The bytes and the token of the object at `key` in `memory`.
async fn object(memory: &MemoryBucket, key: &str) -> (Vec<u8>, RevisionToken) {
    memory.get(&PathBuf::from(format!("/{key}"))).await.unwrap()
}

/// Puts `bytes` at `key` in `memory`, with no condition, its folder made first.
async fn put_object(memory: &MemoryBucket, key: &str, bytes: &[u8]) -> RevisionToken {
    let place = PathBuf::from(format!("/{key}"));
    memory.create_folder(place.parent().unwrap()).await.unwrap();
    memory.put(&place, bytes, &Condition::None).await.unwrap()
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
#[cfg(feature = "minio")]
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
    let keys = keys(&memory).await;
    assert_eq!(keys, ["notes/task.md", "vault/notes/task.md"]);
    let (bytes, _token) = object(&memory, "vault/notes/task.md").await;
    assert_eq!(bytes, b"# Task\n");
}

#[tokio::test]
async fn within_a_folder_keys_land_under_the_prefix_and_the_folder_and_list_without_them() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let picture = path("files/9f86d0");
    let place = path("notes/docs");
    let under_vault = in_vault(&memory, &system).within(&place);
    under_vault
        .write(&picture, PNG, &Precondition::Absent)
        .await
        .unwrap();
    ObjectStore::new(memory.clone(), None, &system)
        .within(&place)
        .write(&picture, PNG, &Precondition::Absent)
        .await
        .unwrap();
    let keys = keys(&memory).await;
    assert_eq!(
        keys,
        ["notes/docs/files/9f86d0", "vault/notes/docs/files/9f86d0"]
    );
    assert_eq!(
        under_vault.list(None).await.unwrap(),
        [path("files/9f86d0")]
    );
    assert_eq!(under_vault.read(&picture).await.unwrap().bytes, PNG);
}

#[tokio::test]
async fn a_read_gives_the_bytes_and_the_tag_unchanged_and_no_object_is_not_found() {
    let system = MemorySystem::new();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    let task = path("notes/task.md");
    let token = put_object(&memory, "vault/notes/task.md", b"# Task\n").await;
    let read = store.read(&task).await.unwrap();
    assert_eq!(read.bytes, b"# Task\n");
    assert_eq!(read.revision, Revision(token.as_str().to_owned()));
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
    let (bytes, token) = object(&memory, "vault/notes/task.md").await;
    assert_eq!(bytes, b"# Done\n");
    assert_eq!(Revision(token.as_str().to_owned()), second);
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
    assert_eq!(keys(&memory).await, Vec::<String>::new());
    store
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    store.delete(&task, &Precondition::None).await.unwrap();
    assert_eq!(keys(&memory).await, Vec::<String>::new());

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
        if let Some(folder) = key.strip_suffix('/') {
            memory
                .create_folder(&PathBuf::from(format!("/{folder}")))
                .await
                .unwrap();
        } else {
            put_object(&memory, key, b"# File\n").await;
        }
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
    put_object(&memory, "vault/a\\b", b"# File\n").await;
    let listed = in_vault(&memory, &system).list(None).await;
    assert_eq!(listed, Err(StoreError::InvalidPath("a\\b".to_owned())));
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
    let down = ObjectStore::new(TestBucket::down(), Some(path("vault")), &system);
    let unsent = down.write(&past, b"# File\n", &Precondition::Absent).await;
    assert_eq!(unsent, refused);
    let keys = keys(&memory).await;
    assert_eq!(keys, [format!("vault/{longest}")]);

    let root = ObjectStore::new(memory.clone(), None, &system);
    root.write(&past, b"# File\n", &Precondition::None)
        .await
        .unwrap();
}

#[tokio::test]
async fn each_failure_of_the_bucket_is_a_backend_error_naming_the_path() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(TestBucket::down(), Some(path("vault")), &system);
    let task = path("notes/task.md");
    let failed = "notes/task.md: no answer for `/vault/notes/task.md`";
    let backend = StoreError::Backend(failed.to_owned());
    assert_eq!(store.read(&task).await, Err(backend.clone()));
    for precondition in [Precondition::None, Precondition::Absent] {
        let written = store.write(&task, b"# Task\n", &precondition).await;
        assert_eq!(written, Err(backend.clone()));
        let deleted = store.delete(&task, &precondition).await;
        assert_eq!(deleted, Err(backend.clone()));
    }
    let notes = store.list(Some(&path("notes"))).await;
    let in_notes = "notes: no answer for `/vault/notes`".to_owned();
    assert_eq!(notes, Err(StoreError::Backend(in_notes)));
    let all = store.list(None).await;
    assert_eq!(
        all,
        Err(StoreError::Backend("no answer for `/vault`".to_owned()))
    );
    let root = ObjectStore::new(TestBucket::down(), None, &system)
        .list(None)
        .await;
    assert_eq!(
        root,
        Err(StoreError::Backend("no answer for `/`".to_owned()))
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
    let keys = keys(&memory).await;
    assert_eq!(
        keys,
        ["vault/notes/media/logo.png", "vault/notes/report.md"]
    );
    let (text, _token) = object(&memory, "vault/notes/report.md").await;
    assert_eq!(text, REPORT.as_bytes());
    let (logo, _logo_revision) = object(&memory, "vault/notes/media/logo.png").await;
    assert_eq!(logo, PNG);
    assert_eq!(get(&store, &report, Missing::Refuse).await.unwrap(), bundle);
}

#[tokio::test]
async fn a_save_under_a_precondition_reads_the_documents_tag_and_not_its_bytes() {
    let system = MemorySystem::new();
    let watched = TestBucket::default();
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
        .seen()
        .into_iter()
        .filter(|read| read.ends_with("/report.md"))
        .collect();
    assert_eq!(of_report, ["head /vault/notes/report.md"; 3]);
}

#[test]
fn a_precondition_is_checked_in_one_step_with_the_write() {
    let system = MemorySystem::new();
    let store = ObjectStore::new(MemoryBucket::default(), None, &system);
    assert!(store.capabilities().atomic_preconditions);
}

#[cfg(feature = "minio")]
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

#[cfg(feature = "minio")]
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
    let (bytes, tag) = object(&memory, "vault/notes/report.md.arky-lock").await;
    assert_eq!(bytes, b"a-guid 1792700000");
    assert!(!store.create(&lock, "another").await.unwrap());
    let found = store.find(&lock).await.unwrap().unwrap();
    assert_eq!(found.holder, "a-guid");
    assert_eq!(found.taken, taken);
    assert_eq!(found.tag, tag.as_str().as_bytes());
}

#[tokio::test]
async fn a_lock_whose_bytes_do_not_read_counts_as_expired_and_the_save_removes_it() {
    let system = MemorySystem::new()
        .with_now(SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000))
        .unwrap();
    let memory = MemoryBucket::default();
    let store = in_vault(&memory, &system);
    for bytes in [b"garbage".as_slice(), b"a-guid soon", b"\xff 12"] {
        put_object(&memory, "vault/notes/report.md.arky-lock", bytes).await;
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
        let keys = keys(&memory).await;
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
    let down = ObjectStore::new(TestBucket::down(), Some(path("vault")), &system);
    let unsent = down
        .write_with_embedded(&past, "# File\n", &files, &Precondition::None)
        .await;
    assert_eq!(unsent, refused);
    assert_eq!(keys(&memory).await, [format!("vault/{longest}")]);
}

#[tokio::test]
async fn a_document_goes_into_each_boxed_store_and_comes_back() {
    let system = MemorySystem::new();
    let stores: Vec<Box<dyn crate::DynStore + '_>> = vec![
        Box::new(FileStore::new(PathBuf::from("/vault"), &system)),
        Box::new(in_vault(&MemoryBucket::default(), &system)),
    ];
    let options = WriteOptions::default();
    let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
    let bundle = BundledDocument::new(parse(REPORT).document, files, Missing::Refuse).unwrap();
    let zip = encode(&bundle, Form::Zip, &options).unwrap();
    let report = path("notes/report.md");
    for store in &stores {
        put(
            store.as_ref(),
            &report,
            Form::Zip,
            &zip,
            &options,
            &Precondition::Absent,
            Missing::Refuse,
        )
        .await
        .unwrap();
        let back = get(store.as_ref(), &report, Missing::Refuse).await.unwrap();
        assert_eq!(back, bundle);
    }
}

#[tokio::test]
async fn ensure_container_asks_the_bucket_to_create_itself() {
    let system = MemorySystem::new();
    let watched = TestBucket::default();
    let store = ObjectStore::new(watched.clone(), None, &system);
    store.ensure_container().await.unwrap();
    assert_eq!(watched.seen(), ["ensure"]);
}

#[tokio::test]
async fn through_a_boxed_store_a_bucket_in_memory_has_a_container_and_no_deletions() {
    let system = MemorySystem::new();
    let store: Box<dyn crate::DynStore + '_> =
        Box::new(in_vault(&MemoryBucket::default(), &system));
    assert_eq!(store.ensure_container().await, Ok(()));
    let no_deletions = Err(StoreError::Unsupported("finish deletions"));
    assert_eq!(store.finish_deletions().await, no_deletions);
}

#[tokio::test]
async fn remove_expired_locks_through_a_boxed_store_removes_an_expired_lock_object() {
    let taken = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000);
    let system = MemorySystem::new().with_now(taken).unwrap();
    let memory = MemoryBucket::default();
    put_object(&memory, "vault/a.md.arky-lock", b"a-guid 1792700000").await;
    system.advance(Duration::from_secs(6)).unwrap();
    let store: Box<dyn crate::DynStore + '_> = Box::new(in_vault(&memory, &system));
    let removed = store.remove_expired_locks(None).await;
    assert_eq!(removed, Ok(vec![lock_of(&path("a.md")).unwrap()]));
    assert_eq!(keys(&memory).await, Vec::<String>::new());
}
