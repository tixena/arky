//! The store over `S3Bucket` on the AWS bucket `just test-s3` names, each test
//! under a key prefix `arky-tests/<uuid>` of its own that it removes at the end.

use alloc::collections::BTreeMap;
use std::env;

use arky::bundle::{BundledDocument, Missing};
use arky::forms::{Form, encode};
use arky::parser::parse;
use arky::writer::WriteOptions;
use os_shim::bucket::{Bucket as _, Condition, S3Bucket, S3Config};
use os_shim::real::RealSystem;
use uuid::Uuid;

use super::{PNG, REPORT, names, path};
use crate::document::{get, put};
use crate::object::{ObjectStore, S3ObjectConfig};
use crate::{Precondition, Store as _, StoreError};

/// The machine the stores of these tests run on.
static MACHINE: RealSystem = RealSystem::new();

/// The test bucket itself, to see and remove every key, locks included.
fn bucket() -> S3Bucket {
    S3Bucket::new(S3Config::new(
        env::var("ARKY_TEST_S3_REGION").unwrap(),
        env::var("ARKY_TEST_S3_BUCKET").unwrap(),
        env::var("AWS_ACCESS_KEY_ID").unwrap(),
        env::var("AWS_SECRET_ACCESS_KEY").unwrap(),
    ))
}

/// Deletes every key under `prefix` and checks that none is left.
async fn remove(prefix: &str) {
    let bucket = bucket();
    let under = format!("{prefix}/");
    for key in bucket.list(&under).await.unwrap() {
        bucket.delete(&key, &Condition::None).await.unwrap();
    }
    assert_eq!(bucket.list(&under).await.unwrap(), Vec::<String>::new());
}

/// A store over the test bucket under a new `arky-tests/<uuid>` prefix, and
/// that prefix.
fn store() -> (ObjectStore<'static, S3Bucket>, String) {
    let prefix = format!("arky-tests/{}", Uuid::new_v4());
    let config = S3ObjectConfig::new(
        env::var("ARKY_TEST_S3_REGION").unwrap(),
        env::var("ARKY_TEST_S3_BUCKET").unwrap(),
        Some(path(&prefix)),
    );
    (ObjectStore::open_s3(&MACHINE, config).unwrap(), prefix)
}

#[tokio::test]
async fn a_document_round_trips_and_a_stale_precondition_is_a_conflict() {
    let (store, prefix) = store();
    let task = path("notes/task.md");
    let first = store
        .write(&task, b"# Task\n", &Precondition::Absent)
        .await
        .unwrap();
    let read = store.read(&task).await.unwrap();
    assert_eq!(read.bytes, b"# Task\n");
    assert_eq!(read.revision, first);
    let conflict = Err(StoreError::Conflict(task.clone()));
    let again = store.write(&task, b"# B\n", &Precondition::Absent).await;
    assert_eq!(again, conflict);
    let second = store
        .write(&task, b"# Done\n", &Precondition::Matches(first.clone()))
        .await
        .unwrap();
    let stale = store
        .write(&task, b"# B\n", &Precondition::Matches(first))
        .await;
    assert_eq!(stale, conflict);
    let kept = store.read(&task).await.unwrap();
    assert_eq!(kept.bytes, b"# Done\n");
    assert_eq!(kept.revision, second);
    remove(&prefix).await;
}

#[tokio::test]
async fn a_save_with_an_embedded_picture_reads_back_and_leaves_no_lock() {
    let (store, prefix) = store();
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
        &Precondition::Absent,
        Missing::Refuse,
    )
    .await
    .unwrap();
    assert_eq!(get(&store, &report, Missing::Refuse).await.unwrap(), bundle);
    let keys = bucket().list(&format!("{prefix}/")).await.unwrap();
    assert_eq!(
        keys,
        [
            format!("{prefix}/notes/media/logo.png"),
            format!("{prefix}/notes/report.md"),
        ]
    );
    remove(&prefix).await;
}

#[tokio::test]
async fn a_listing_is_sorted_and_a_delete_removes_the_object() {
    let (store, prefix) = store();
    for name in ["notes/b.md", "notesx/a.md", "notes/a/c.md", "Z.md"] {
        store
            .write(&path(name), b"# File\n", &Precondition::None)
            .await
            .unwrap();
    }
    let all = store.list(None).await.unwrap();
    assert_eq!(
        names(&all),
        ["Z.md", "notes/a/c.md", "notes/b.md", "notesx/a.md"]
    );
    let notes = store.list(Some(&path("notes"))).await.unwrap();
    assert_eq!(names(&notes), ["notes/a/c.md", "notes/b.md"]);

    let top = path("Z.md");
    store.delete(&top, &Precondition::None).await.unwrap();
    assert_eq!(store.read(&top).await, Err(StoreError::NotFound(top)));
    let note = path("notes/b.md");
    let current = store.read(&note).await.unwrap().revision;
    store
        .delete(&note, &Precondition::Matches(current))
        .await
        .unwrap();
    assert_eq!(store.read(&note).await, Err(StoreError::NotFound(note)));
    let left = store.list(None).await.unwrap();
    assert_eq!(names(&left), ["notes/a/c.md", "notesx/a.md"]);
    remove(&prefix).await;
}

#[tokio::test]
async fn ensure_leaves_the_existing_bucket_as_it_is() {
    let (store, prefix) = store();
    let note = path("notes/a.md");
    store
        .write(&note, b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    store.ensure(None).await.unwrap();
    assert_eq!(store.list(None).await.unwrap(), [note]);
    remove(&prefix).await;
}
