//! Each call to `AnyStore` goes to the store it holds.

use alloc::collections::BTreeMap;
use core::time::Duration;
use std::path::PathBuf;

use os_shim::mock::MemorySystem;

use crate::any::AnyStore;
use crate::file::FileStore;
use crate::lock::lock_of;
use crate::{Precondition, Store as _, StoreError, StorePath};

#[tokio::test]
async fn each_call_goes_to_the_store_it_holds() {
    let system = MemorySystem::new()
        .with_file("/vault/notes/a.md", b"# A\n")
        .unwrap();
    let store = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &system));
    let file = FileStore::new(PathBuf::from("/vault"), &system);
    let a = StorePath::new("notes/a.md").unwrap();
    let b = StorePath::new("notes/b.md").unwrap();

    assert_eq!(store.capabilities(), file.capabilities());
    assert_eq!(store.read(&a).await, file.read(&a).await);
    let written = store.write(&b, b"# B\n", &Precondition::Absent).await;
    assert_eq!(written, Ok(file.read(&b).await.unwrap().revision));
    assert_eq!(store.list(None).await.unwrap(), [a.clone(), b.clone()]);
    store.delete(&a, &Precondition::None).await.unwrap();
    assert_eq!(file.list(None).await.unwrap(), [b]);
}

#[tokio::test]
async fn each_call_with_embedded_files_goes_to_the_store_it_holds() {
    let system = MemorySystem::new();
    let store = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &system));
    let file = FileStore::new(PathBuf::from("/vault"), &system);
    let a = StorePath::new("notes/a.md").unwrap();
    let logo = StorePath::new("notes/logo.png").unwrap();
    let files = BTreeMap::from([(logo.clone(), b"logo".to_vec())]);
    let written = store
        .write_with_embedded(&a, "![Logo](logo.png)\n", &files, &Precondition::Absent)
        .await;
    assert_eq!(written, Ok(file.read(&a).await.unwrap().revision));
    assert_eq!(file.list(None).await.unwrap(), [a.clone(), logo.clone()]);
    let read = store.read_embedded(&a, &logo).await;
    assert_eq!(read, file.read_embedded(&a, &logo).await);
    assert_eq!(read, Ok(b"logo".to_vec()));
}

#[tokio::test]
async fn each_store_operation_outside_files_goes_to_the_store_it_holds() {
    let system = MemorySystem::new()
        .with_file("/vault/a.md.arky-lock", b"a-guid")
        .unwrap();
    system.advance(Duration::from_secs(6)).unwrap();
    let store = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &system));
    let no_container = Err(StoreError::Unsupported("create its container"));
    assert_eq!(store.ensure_container().await, no_container);
    let no_deletions = Err(StoreError::Unsupported("finish deletions"));
    assert_eq!(store.finish_deletions().await, no_deletions);
    let lock = lock_of(&StorePath::new("a.md").unwrap()).unwrap();
    assert_eq!(store.remove_expired_locks(None).await, Ok(vec![lock]));
}
