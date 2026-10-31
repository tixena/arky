//! Store paths, the path of a name beside one, the messages of store
//! errors, and a store held as a `Box<dyn DynStore>`.

#[cfg(feature = "minio")]
use alloc::sync::Arc;
#[cfg(feature = "minio")]
use core::future;
#[cfg(feature = "minio")]
use std::path::Path;
use std::path::PathBuf;
#[cfg(feature = "minio")]
use std::sync::Mutex;

use arky::bundle::Missing;
use arky::forms::Form;
use arky::writer::{WriteOptions, write};
#[cfg(feature = "minio")]
use futures_io::{AsyncRead, AsyncWrite};
#[cfg(feature = "minio")]
use os_shim::bucket::{Bucket, Condition, Depth, Guarantees, MemoryBucket, RevisionToken};
#[cfg(feature = "minio")]
use os_shim::error::{ErrorKind, Result as StorageResult, StorageError};
use os_shim::mock::MemorySystem;
#[cfg(feature = "minio")]
use os_shim::{BoxFuture, FileMetadata, WalkEntry};

use crate::file::FileStore;
#[cfg(feature = "minio")]
use crate::object::EnsureBucket;
use crate::{DynStore, Precondition, StoreError, StorePath, document};

/// A `MemoryBucket` that notes each `get`, `head` and `ensure_bucket` it
/// answers; made by `down`, a bucket whose every call fails as an unreachable
/// server's would.
#[cfg(feature = "minio")]
#[derive(Clone, Default)]
pub struct TestBucket {
    bucket: MemoryBucket,
    down: bool,
    seen: Arc<Mutex<Vec<String>>>,
}

#[cfg(feature = "minio")]
impl TestBucket {
    /// `Other` naming `path` when the bucket is down.
    fn answer(&self, path: &Path) -> StorageResult<()> {
        if self.down {
            let message = format!("no answer for `{}`", path.display());
            return Err(StorageError::new(ErrorKind::Other, message));
        }
        Ok(())
    }

    pub fn down() -> Self {
        Self {
            down: true,
            ..Self::default()
        }
    }

    fn note(&self, call: String) {
        self.seen.lock().unwrap().push(call);
    }

    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

#[cfg(feature = "minio")]
impl Bucket for TestBucket {
    fn append<'call>(
        &'call self,
        path: &'call Path,
        more: &'call [u8],
    ) -> BoxFuture<'call, StorageResult<RevisionToken>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.append(path, more).await
        })
    }

    fn append_stream<'bucket, 'call>(
        &'bucket self,
        path: &'call Path,
    ) -> BoxFuture<'call, StorageResult<Box<dyn AsyncWrite + Send + Unpin + 'bucket>>>
    where
        'bucket: 'call,
    {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.append_stream(path).await
        })
    }

    fn canonicalize<'call>(
        &'call self,
        path: &'call Path,
    ) -> BoxFuture<'call, StorageResult<PathBuf>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.canonicalize(path).await
        })
    }

    fn copy<'call>(
        &'call self,
        from: &'call Path,
        to: &'call Path,
    ) -> BoxFuture<'call, StorageResult<u64>> {
        Box::pin(async move {
            self.answer(from)?;
            self.bucket.copy(from, to).await
        })
    }

    fn create_folder<'call>(&'call self, path: &'call Path) -> BoxFuture<'call, StorageResult<()>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.create_folder(path).await
        })
    }

    fn delete<'call>(
        &'call self,
        path: &'call Path,
        condition: &'call Condition,
    ) -> BoxFuture<'call, StorageResult<()>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.delete(path, condition).await
        })
    }

    fn get<'call>(
        &'call self,
        path: &'call Path,
    ) -> BoxFuture<'call, StorageResult<(Vec<u8>, RevisionToken)>> {
        Box::pin(async move {
            self.answer(path)?;
            self.note(format!("get {}", path.display()));
            self.bucket.get(path).await
        })
    }

    /// A down bucket keeps no folders, as a server's bucket keeps none.
    fn guarantees(&self) -> Guarantees {
        if self.down {
            Guarantees::none()
        } else {
            self.bucket.guarantees()
        }
    }

    fn head<'call>(
        &'call self,
        path: &'call Path,
    ) -> BoxFuture<'call, StorageResult<FileMetadata>> {
        Box::pin(async move {
            self.answer(path)?;
            self.note(format!("head {}", path.display()));
            self.bucket.head(path).await
        })
    }

    fn list<'call>(
        &'call self,
        folder: &'call Path,
        depth: Depth,
    ) -> BoxFuture<'call, StorageResult<Vec<WalkEntry>>> {
        Box::pin(async move {
            self.answer(folder)?;
            self.bucket.list(folder, depth).await
        })
    }

    fn put<'call>(
        &'call self,
        path: &'call Path,
        bytes: &'call [u8],
        condition: &'call Condition,
    ) -> BoxFuture<'call, StorageResult<RevisionToken>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.put(path, bytes, condition).await
        })
    }

    fn read_stream<'bucket, 'call>(
        &'bucket self,
        path: &'call Path,
    ) -> BoxFuture<'call, StorageResult<Box<dyn AsyncRead + Send + Unpin + 'bucket>>>
    where
        'bucket: 'call,
    {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.read_stream(path).await
        })
    }

    fn remove_folder<'call>(&'call self, path: &'call Path) -> BoxFuture<'call, StorageResult<()>> {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.remove_folder(path).await
        })
    }

    fn rename<'call>(
        &'call self,
        from: &'call Path,
        to: &'call Path,
    ) -> BoxFuture<'call, StorageResult<()>> {
        Box::pin(async move {
            self.answer(from)?;
            self.bucket.rename(from, to).await
        })
    }

    fn walk<'call>(
        &'call self,
        folder: &'call Path,
        follow_links: bool,
        hidden: bool,
    ) -> BoxFuture<'call, StorageResult<Vec<WalkEntry>>> {
        Box::pin(async move {
            self.answer(folder)?;
            self.bucket.walk(folder, follow_links, hidden).await
        })
    }

    fn write_stream<'bucket, 'call>(
        &'bucket self,
        path: &'call Path,
        condition: Condition,
    ) -> BoxFuture<'call, StorageResult<Box<dyn AsyncWrite + Send + Unpin + 'bucket>>>
    where
        'bucket: 'call,
    {
        Box::pin(async move {
            self.answer(path)?;
            self.bucket.write_stream(path, condition).await
        })
    }
}

#[cfg(feature = "minio")]
impl EnsureBucket for TestBucket {
    fn ensure_bucket(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
        if self.down {
            return future::ready(Err(StoreError::Backend("no answer".to_owned())));
        }
        self.note("ensure".to_owned());
        future::ready(Ok(()))
    }
}

#[test]
fn a_path_of_parts_is_a_store_path() {
    for text in [
        "a.md",
        "notes/task.md",
        ".obsidian/x.md",
        "..a/b.",
        "a b/c d.md",
    ] {
        assert_eq!(StorePath::new(text).unwrap().as_str(), text);
    }
}

#[test]
fn a_path_that_breaks_a_rule_is_invalid() {
    let broken = [
        "",
        "a//b",
        "/a",
        "a/",
        "/",
        "a\\b",
        "./a",
        ".",
        "..",
        "a/./b",
        "notes/../secret.md",
    ];
    for text in broken {
        let refused = Err(StoreError::InvalidPath(text.to_owned()));
        assert_eq!(StorePath::new(text), refused, "{text:?}");
    }
}

#[test]
fn a_name_beside_a_path_is_its_path_from_the_folder_of_the_path() {
    let cases = [
        ("notes/report.md", "media/logo.png", "notes/media/logo.png"),
        ("report.md", "logo.png", "logo.png"),
        ("a/b/c.md", "parts/d.md", "a/b/parts/d.md"),
    ];
    for (path, name, beside) in cases {
        let found = StorePath::new(path).unwrap().beside(name).unwrap();
        assert_eq!(found.as_str(), beside, "{path} {name}");
    }
}

#[test]
fn a_name_beside_a_path_that_breaks_a_rule_is_invalid() {
    let path = StorePath::new("notes/report.md").unwrap();
    for (name, beside) in [
        ("../x.png", "notes/../x.png"),
        ("a\\b.png", "notes/a\\b.png"),
    ] {
        let refused = Err(StoreError::InvalidPath(beside.to_owned()));
        assert_eq!(path.beside(name), refused, "{name:?}");
    }
}

#[test]
fn an_error_names_its_path() {
    let path = StorePath::new("notes/none.md").unwrap();
    let missing = StoreError::NotFound(path.clone()).to_string();
    assert_eq!(missing, "notes/none.md: no file at this path");
    let conflict = StoreError::Conflict(path).to_string();
    assert_eq!(
        conflict,
        "notes/none.md: the file changed, or a file is already there"
    );
    let invalid = StoreError::InvalidPath("a//b".to_owned()).to_string();
    assert_eq!(invalid, "`a//b` is not a store path");
}

#[tokio::test]
async fn a_boxed_file_store_takes_the_document_functions_and_has_no_container_and_no_deletions() {
    let system = MemorySystem::new();
    let store: Box<dyn DynStore + '_> = Box::new(FileStore::new(PathBuf::from("/notes"), &system));
    let path = StorePath::new("a.md").unwrap();
    let options = WriteOptions::default();
    document::put(
        store.as_ref(),
        &path,
        Form::Markdown,
        b"# A\n\n* item\n",
        &options,
        &Precondition::Absent,
        Missing::Refuse,
    )
    .await
    .unwrap();
    let back = document::get(store.as_ref(), &path, Missing::Refuse)
        .await
        .unwrap();
    assert_eq!(write(back.document(), &options).unwrap(), "# A\n\n- item\n");

    let no_container = Err(StoreError::Unsupported("create its container"));
    assert_eq!(store.ensure_container().await, no_container);
    let no_deletions = Err(StoreError::Unsupported("finish deletions"));
    assert_eq!(store.finish_deletions().await, no_deletions);
    assert_eq!(store.remove_expired_locks(None).await, Ok(Vec::new()));
}
