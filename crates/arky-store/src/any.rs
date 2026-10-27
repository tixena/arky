//! One type for whichever store a caller opened.

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
#[non_exhaustive]
pub enum AnyStore<'system> {
    File(FileStore<'system>),
    #[cfg(feature = "minio")]
    Minio(ObjectStore<'system, MinioBucket>),
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
