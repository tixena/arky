//! One type for whichever store a caller opened.
//!
//! A program that picks its store from configuration at run time keeps the
//! store in an [`AnyStore`] and calls the same [`Store`] methods and
//! [`document`](crate::document) functions whatever it picked.

use alloc::collections::BTreeMap;

#[cfg(feature = "minio")]
use os_shim::bucket::MinioBucket;

use crate::file::FileStore;
#[cfg(feature = "mongodb")]
use crate::mongo::MongoStore;
#[cfg(feature = "minio")]
use crate::object::ObjectStore;
use crate::{Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored};

/// Whichever store a caller opened. Each variant is behind its store's
/// cargo feature.
///
/// It implements [`Store`] by passing each call to the store it holds.
#[cfg_attr(
    not(feature = "minio"),
    doc = r##"
```rust
use std::path::PathBuf;

use arky_store::any::AnyStore;
use arky_store::file::FileStore;
use arky_store::{Precondition, Store as _, StorePath};
use os_shim::mock::MemorySystem;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::new();
    let store = AnyStore::File(FileStore::new(PathBuf::from("/notes"), &system));
    let path = StorePath::new("ideas.md")?;
    store.write(&path, b"# Ideas\n", &Precondition::Absent).await?;
    assert_eq!(store.read(&path).await?.bytes, b"# Ideas\n");
    assert!(!store.capabilities().atomic_preconditions);
    Ok(())
}
```
"##
)]
#[cfg_attr(
    feature = "minio",
    doc = r##"
Here `open` picks the store a target names, `file:<root>` or
`minio:<bucket>`, and the program calls the same methods whatever it picked:

```rust
use std::path::PathBuf;

use arky_store::any::AnyStore;
use arky_store::file::FileStore;
use arky_store::object::{ObjectConfig, ObjectStore};
use arky_store::{Precondition, Store as _, StoreError, StorePath};
use os_shim::System;
use os_shim::mock::MemorySystem;

fn open<'s>(target: &str, system: &'s dyn System) -> Result<AnyStore<'s>, StoreError> {
    if let Some(root) = target.strip_prefix("file:") {
        return Ok(AnyStore::File(FileStore::new(PathBuf::from(root), system)));
    }
    let Some(bucket) = target.strip_prefix("minio:") else {
        return Err(StoreError::Backend(format!("no store for `{target}`")));
    };
    let config = ObjectConfig::new(
        "http://127.0.0.1:9000".to_owned(),
        "us-east-1".to_owned(),
        bucket.to_owned(),
        None,
    );
    Ok(AnyStore::Minio(ObjectStore::open(system, config)?))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::new();
    let store = open("file:/notes", &system)?;
    let path = StorePath::new("ideas.md")?;
    store.write(&path, b"# Ideas\n", &Precondition::Absent).await?;
    assert_eq!(store.read(&path).await?.bytes, b"# Ideas\n");
    assert!(!store.capabilities().atomic_preconditions);
    Ok(())
}
```
"##
)]
#[non_exhaustive]
pub enum AnyStore<'system> {
    /// The file store.
    File(FileStore<'system>),
    /// The `MinIO` store, over a bucket on a server. It needs the `minio`
    /// feature.
    #[cfg(feature = "minio")]
    Minio(ObjectStore<'system, MinioBucket>),
    /// The `MongoDB` store. It needs the `mongodb` feature.
    #[cfg(feature = "mongodb")]
    Mongo(MongoStore<'system>),
}

impl Store for AnyStore<'_> {
    fn capabilities(&self) -> Capabilities {
        match self {
            Self::File(store) => store.capabilities(),
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.capabilities(),
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.capabilities(),
        }
    }

    async fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        match self {
            Self::File(store) => store.delete(path, precondition).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.delete(path, precondition).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.delete(path, precondition).await,
        }
    }

    async fn ensure_container(&self) -> Result<(), StoreError> {
        match self {
            Self::File(store) => Store::ensure_container(store).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => Store::ensure_container(store).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => Store::ensure_container(store).await,
        }
    }

    async fn finish_deletions(&self) -> Result<Vec<StorePath>, StoreError> {
        match self {
            Self::File(store) => Store::finish_deletions(store).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => Store::finish_deletions(store).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => Store::finish_deletions(store).await,
        }
    }

    async fn list(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        match self {
            Self::File(store) => store.list(prefix).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.list(prefix).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.list(prefix).await,
        }
    }

    async fn read(&self, path: &StorePath) -> Result<Stored, StoreError> {
        match self {
            Self::File(store) => store.read(path).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.read(path).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.read(path).await,
        }
    }

    async fn read_embedded(
        &self,
        document: &StorePath,
        file: &StorePath,
    ) -> Result<Vec<u8>, StoreError> {
        match self {
            Self::File(store) => store.read_embedded(document, file).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.read_embedded(document, file).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.read_embedded(document, file).await,
        }
    }

    async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        match self {
            Self::File(store) => Store::remove_expired_locks(store, under).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => Store::remove_expired_locks(store, under).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => Store::remove_expired_locks(store, under).await,
        }
    }

    async fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        match self {
            Self::File(store) => store.write(path, bytes, precondition).await,
            #[cfg(feature = "minio")]
            Self::Minio(store) => store.write(path, bytes, precondition).await,
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => store.write(path, bytes, precondition).await,
        }
    }

    async fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        match self {
            Self::File(store) => {
                store
                    .write_with_embedded(path, text, files, precondition)
                    .await
            }
            #[cfg(feature = "minio")]
            Self::Minio(store) => {
                store
                    .write_with_embedded(path, text, files, precondition)
                    .await
            }
            #[cfg(feature = "mongodb")]
            Self::Mongo(store) => {
                store
                    .write_with_embedded(path, text, files, precondition)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests;
