//! Each scenario runs over `MemorySystem` and over `RealSystem` in a
//! temporary folder.

use alloc::collections::BTreeMap;
use core::time::Duration;
use std::path::{Path, PathBuf};
use std::process;

use os_shim::System;
use os_shim::bucket::Condition;
use os_shim::mock::MemorySystem;
use os_shim::real::RealSystem;
use tokio::runtime::Builder;

use crate::file::FileStore;
use crate::lock::{Locks as _, lock_of};
use crate::{Precondition, Revision, Store as _, StoreError, StorePath};

/// Runs `scenario` over a folder of each system.
fn on_both<S>(scenario: &S)
where
    S: AsyncFn(&dyn System, &Path),
{
    let runtime = Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        scenario(&MemorySystem::new(), Path::new("/vault")).await;
        let real = RealSystem::new();
        let folder = real.create_temp_dir().await.unwrap();
        scenario(&real, folder.path()).await;
    });
}

/// Every file under `root`, hidden ones included.
async fn everything(system: &dyn System, root: &Path) -> Vec<String> {
    let store = FileStore::new(root.to_path_buf(), system).listing(false, true);
    let paths = store.list(None).await.unwrap();
    paths.iter().map(|path| path.as_str().to_owned()).collect()
}

/// A store over `root` holding `files`, each with its own path as its bytes.
async fn holding<'system>(
    system: &'system dyn System,
    root: &Path,
    files: &[&str],
) -> FileStore<'system> {
    let store = FileStore::new(root.to_path_buf(), system);
    for file in files {
        let path = StorePath::new(file).unwrap();
        store
            .write(&path, file.as_bytes(), &Precondition::None)
            .await
            .unwrap();
    }
    store
}

fn names(paths: &[StorePath]) -> Vec<&str> {
    paths.iter().map(StorePath::as_str).collect()
}

/// Compiles only for a value that can go to another thread.
const fn sent<T>(value: T) -> T
where
    T: Send,
{
    value
}

async fn sample(system: &dyn System, root: &Path) {
    let store = FileStore::new(root.to_path_buf(), system);
    let path = StorePath::new("notes/task.md").unwrap();
    let first = store
        .write(&path, b"# Task\n", &Precondition::Absent)
        .await
        .unwrap();
    assert!(first.0.starts_with("sha256:"));
    let stored = store.read(&path).await.unwrap();
    assert_eq!(stored.bytes, b"# Task\n");
    store
        .write(&path, b"# Done\n", &Precondition::Matches(first.clone()))
        .await
        .unwrap();
    assert_eq!(
        store
            .write(&path, b"# Again\n", &Precondition::Matches(first))
            .await,
        Err(StoreError::Conflict(path.clone()))
    );
    assert_eq!(
        StorePath::new("notes/../secret.md"),
        Err(StoreError::InvalidPath("notes/../secret.md".into()))
    );
    assert_eq!(everything(system, root).await, ["notes/task.md"]);
}

#[test]
fn the_sample_holds_and_leaves_no_temporary_file() {
    on_both(&sample);
}

async fn revision_is_the_hash(system: &dyn System, root: &Path) {
    let store = FileStore::new(root.to_path_buf(), system);
    let path = StorePath::new("abc.md").unwrap();
    let written = store
        .write(&path, b"abc", &Precondition::None)
        .await
        .unwrap();
    let hash = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(written, Revision(hash.to_owned()));
    assert_eq!(store.read(&path).await.unwrap().revision, written);
}

#[test]
fn a_revision_is_the_sha256_of_the_bytes() {
    on_both(&revision_is_the_hash);
}

async fn writes_by_precondition(system: &dyn System, root: &Path) {
    let store = holding(system, root, &["a.md"]).await;
    let present = StorePath::new("a.md").unwrap();
    let missing = StorePath::new("b.md").unwrap();
    let old = store.read(&present).await.unwrap();

    let absent = store.write(&present, b"new", &Precondition::Absent).await;
    assert_eq!(absent, Err(StoreError::Conflict(present.clone())));
    let stale = Precondition::Matches(Revision("sha256:00".to_owned()));
    assert_eq!(
        store.write(&present, b"new", &stale).await,
        Err(StoreError::Conflict(present.clone()))
    );
    let matched = Precondition::Matches(old.revision.clone());
    assert_eq!(
        store.write(&missing, b"new", &matched).await,
        Err(StoreError::Conflict(missing.clone()))
    );
    assert_eq!(store.read(&present).await.unwrap(), old);
    assert_eq!(everything(system, root).await, ["a.md"]);

    store
        .write(&missing, b"b", &Precondition::Absent)
        .await
        .unwrap();
    let current = store.write(&present, b"a", &matched).await.unwrap();
    store
        .write(&present, b"none", &Precondition::None)
        .await
        .unwrap();
    store
        .write(&missing, b"none", &Precondition::None)
        .await
        .unwrap();
    assert_eq!(store.read(&present).await.unwrap().bytes, b"none");
    assert_eq!(store.read(&missing).await.unwrap().bytes, b"none");
    assert_ne!(store.read(&present).await.unwrap().revision, current);
}

#[test]
fn a_write_goes_ahead_only_when_its_precondition_holds() {
    on_both(&writes_by_precondition);
}

async fn deletes_by_precondition(system: &dyn System, root: &Path) {
    let store = holding(system, root, &["a.md"]).await;
    let present = StorePath::new("a.md").unwrap();
    let old = store.read(&present).await.unwrap();

    let absent = store.delete(&present, &Precondition::Absent).await;
    assert_eq!(absent, Err(StoreError::Conflict(present.clone())));
    let stale = Precondition::Matches(Revision("sha256:00".to_owned()));
    let refused = store.delete(&present, &stale).await;
    assert_eq!(refused, Err(StoreError::Conflict(present.clone())));
    assert_eq!(store.read(&present).await.unwrap(), old);

    let matched = Precondition::Matches(old.revision);
    store.delete(&present, &matched).await.unwrap();
    assert_eq!(everything(system, root).await, Vec::<String>::new());
    assert_eq!(
        store.delete(&present, &matched).await,
        Err(StoreError::Conflict(present.clone()))
    );
    assert_eq!(
        store.delete(&present, &Precondition::None).await,
        Err(StoreError::NotFound(present.clone()))
    );
}

#[test]
fn a_delete_goes_ahead_only_when_its_precondition_holds() {
    on_both(&deletes_by_precondition);
}

async fn missing_file(system: &dyn System, root: &Path) {
    let store = FileStore::new(root.to_path_buf(), system);
    let path = StorePath::new("notes/none.md").unwrap();
    let error = store.read(&path).await.unwrap_err();
    assert_eq!(error, StoreError::NotFound(path));
    assert_eq!(error.to_string(), "notes/none.md: no file at this path");
}

#[test]
fn a_read_of_a_missing_file_is_not_found() {
    on_both(&missing_file);
}

async fn below_a_file(system: &dyn System, root: &Path) {
    let store = holding(system, root, &["a.md"]).await;
    let below = StorePath::new("a.md/b.md").unwrap();
    assert_eq!(
        store.read(&below).await,
        Err(StoreError::NotFound(below.clone()))
    );
    assert_eq!(
        store.delete(&below, &Precondition::None).await,
        Err(StoreError::NotFound(below.clone()))
    );
    let stale = Precondition::Matches(Revision("sha256:00".to_owned()));
    assert_eq!(
        store.delete(&below, &stale).await,
        Err(StoreError::Conflict(below.clone()))
    );
    assert_eq!(everything(system, root).await, ["a.md"]);
}

#[test]
fn a_path_below_a_file_has_no_file() {
    on_both(&below_a_file);
}

async fn hidden_left_out(system: &dyn System, root: &Path) {
    let store = holding(system, root, &["a.md", "b/c.md", ".obsidian/x.md"]).await;
    assert_eq!(names(&store.list(None).await.unwrap()), ["a.md", "b/c.md"]);
}

#[test]
fn a_listing_leaves_hidden_entries_out_by_default() {
    on_both(&hidden_left_out);
}

async fn hidden_listed(system: &dyn System, root: &Path) {
    let files = ["a.md", "b/c.md", ".obsidian/x.md"];
    let store = holding(system, root, &files).await.listing(false, true);
    let listed = store.list(None).await.unwrap();
    assert_eq!(names(&listed), [".obsidian/x.md", "a.md", "b/c.md"]);
}

#[test]
fn a_listing_of_hidden_entries_lists_them() {
    on_both(&hidden_listed);
}

async fn listed_in_byte_order(system: &dyn System, root: &Path) {
    let files = ["b/d/e.md", "b/c.md", "b.md", "a.md", "B.md", "bc/f.md"];
    let store = holding(system, root, &files).await;
    let every = ["B.md", "a.md", "b.md", "b/c.md", "b/d/e.md", "bc/f.md"];
    assert_eq!(names(&store.list(None).await.unwrap()), every);
    let prefix = StorePath::new("b").unwrap();
    let under = store.list(Some(&prefix)).await.unwrap();
    assert_eq!(names(&under), ["b/c.md", "b/d/e.md"]);
    let none = StorePath::new("z").unwrap();
    assert_eq!(store.list(Some(&none)).await, Ok(Vec::new()));
}

#[test]
fn a_listing_gives_every_file_under_the_prefix_in_byte_order() {
    on_both(&listed_in_byte_order);
}

#[test]
fn two_writes_in_one_process_use_two_temporary_names() {
    let folder = Path::new("/vault/notes");
    let first = FileStore::temporary(folder, "task.md");
    let second = FileStore::temporary(folder, "task.md");
    assert_ne!(first, second);
    let prefix = format!(".task.md.arky-write-{}-", process::id());
    for name in [&first, &second] {
        let file = name.file_name().unwrap().to_str().unwrap();
        assert!(file.starts_with(&prefix), "{file}");
    }
}

#[tokio::test]
async fn a_failed_write_leaves_no_temporary_file() {
    let real = RealSystem::new();
    let folder = real.create_temp_dir().await.unwrap();
    let store = holding(&real, folder.path(), &["notes/a.md"]).await;
    let onto_folder = StorePath::new("notes").unwrap();
    let failed = store.write(&onto_folder, b"x", &Precondition::None).await;
    assert!(matches!(failed, Err(StoreError::Backend(_))), "{failed:?}");
    assert_eq!(everything(&real, folder.path()).await, ["notes/a.md"]);
}

#[tokio::test]
async fn a_write_below_a_file_fails_and_leaves_the_file() {
    let real = RealSystem::new();
    let folder = real.create_temp_dir().await.unwrap();
    let store = holding(&real, folder.path(), &["a.md"]).await;
    let below = StorePath::new("a.md/b.md").unwrap();
    let failed = store.write(&below, b"x", &Precondition::Absent).await;
    assert!(matches!(failed, Err(StoreError::Backend(_))), "{failed:?}");
    assert_eq!(everything(&real, folder.path()).await, ["a.md"]);
}

#[tokio::test]
async fn a_read_can_be_sent_to_another_thread() {
    let system = MemorySystem::new()
        .with_file("/vault/a.md", b"# A\n")
        .unwrap();
    let store = FileStore::new(PathBuf::from("/vault"), &system);
    let path = StorePath::new("a.md").unwrap();
    let stored = sent(store.read(&path)).await.unwrap();
    assert_eq!(stored.bytes, b"# A\n");
}

#[test]
fn the_preconditions_are_not_atomic() {
    let system = MemorySystem::new();
    let store = FileStore::new(PathBuf::from("/vault"), &system);
    assert!(!store.capabilities().atomic_preconditions);
}

#[tokio::test]
async fn a_save_in_a_folder_that_does_not_exist_yet_takes_its_lock_there() {
    let system = MemorySystem::new();
    let store = FileStore::new(PathBuf::from("/vault"), &system);
    let report = StorePath::new("new/report.md").unwrap();
    let logo = StorePath::new("new/media/logo.png").unwrap();
    let files = BTreeMap::from([(logo.clone(), b"logo".to_vec())]);
    store
        .write_with_embedded(
            &report,
            "![Logo](media/logo.png)\n",
            &files,
            &Precondition::Absent,
        )
        .await
        .unwrap();
    assert_eq!(
        names(&store.list(None).await.unwrap()),
        [logo.as_str(), report.as_str()]
    );
    assert!(
        !system
            .exists(Path::new("/vault/new/report.md.arky-lock"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_lock_is_a_file_holding_the_guid_and_one_that_is_not_text_never_names_a_guid() {
    let system = MemorySystem::new();
    let store = FileStore::new(PathBuf::from("/vault"), &system);
    let lock = lock_of(&StorePath::new("notes/report.md").unwrap()).unwrap();
    assert!(store.create(&lock, "a-guid").await.unwrap());
    let file = Path::new("/vault/notes/report.md.arky-lock");
    assert_eq!(system.read_to_string(file).await.unwrap(), "a-guid");
    assert!(!store.create(&lock, "another").await.unwrap());
    let found = store.find(&lock).await.unwrap().unwrap();
    let (_bytes, token) = system.read(file).await.unwrap();
    assert_eq!(
        (found.holder.as_str(), found.tag.as_slice()),
        ("a-guid", token.as_str().as_bytes())
    );

    system
        .write(file, b"\xff\xfe", Condition::None)
        .await
        .unwrap();
    let unreadable = store.find(&lock).await.unwrap().unwrap();
    assert_ne!(unreadable.holder, "a-guid");
    let (_unreadable, rewritten) = system.read(file).await.unwrap();
    assert_eq!(unreadable.tag, rewritten.as_str().as_bytes());
    assert!(store.remove_if(&lock, &unreadable).await.unwrap());
    assert!(store.find(&lock).await.unwrap().is_none());
    assert!(!store.remove_if(&lock, &unreadable).await.unwrap());
}

#[tokio::test]
async fn remove_expired_locks_through_a_boxed_store_removes_an_expired_lock() {
    let system = MemorySystem::new()
        .with_file("/vault/a.md.arky-lock", b"a-guid")
        .unwrap();
    system.advance(Duration::from_secs(6)).unwrap();
    let store: Box<dyn crate::DynStore + '_> =
        Box::new(FileStore::new(PathBuf::from("/vault"), &system));
    let lock = lock_of(&StorePath::new("a.md").unwrap()).unwrap();
    assert_eq!(store.remove_expired_locks(None).await, Ok(vec![lock]));
    assert!(
        !system
            .exists(Path::new("/vault/a.md.arky-lock"))
            .await
            .unwrap()
    );
}
