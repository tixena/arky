//! Store paths, the path of a name beside one, the messages of store
//! errors, and a store held as a `Box<dyn DynStore>`.

#[cfg(any(feature = "minio", feature = "s3"))]
use alloc::sync::Arc;
#[cfg(any(feature = "minio", feature = "s3"))]
use core::future;
#[cfg(any(feature = "minio", feature = "s3"))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(any(feature = "minio", feature = "s3"))]
use std::sync::Mutex;

use arky::bundle::Missing;
use arky::forms::Form;
use arky::writer::{WriteOptions, write};
#[cfg(any(feature = "minio", feature = "s3"))]
use futures_io::{AsyncRead, AsyncWrite};
#[cfg(any(feature = "minio", feature = "s3"))]
use os_shim::bucket::{Bucket, Condition, Depth, Guarantees, MemoryBucket, RevisionToken};
#[cfg(any(feature = "minio", feature = "s3"))]
use os_shim::error::{ErrorKind, Result as StorageResult, StorageError};
use os_shim::mock::MemorySystem;
#[cfg(any(feature = "minio", feature = "s3"))]
use os_shim::{FileMetadata, WalkEntry};

use crate::file::FileStore;
#[cfg(any(feature = "minio", feature = "s3"))]
use crate::object::EnsureBucket;
use crate::{DynStore, Precondition, StoreError, StorePath, document};

/// A `MemoryBucket` that notes each `get`, `head` and `ensure_bucket` it
/// answers; made by `down`, a bucket whose every call fails as an unreachable
/// server's would.
#[cfg(any(feature = "minio", feature = "s3"))]
#[derive(Clone, Default)]
pub struct TestBucket {
    bucket: MemoryBucket,
    down: bool,
    seen: Arc<Mutex<Vec<String>>>,
}

#[cfg(any(feature = "minio", feature = "s3"))]
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

#[cfg(any(feature = "minio", feature = "s3"))]
impl Bucket for TestBucket {
    async fn append(&self, path: &Path, more: &[u8]) -> StorageResult<RevisionToken> {
        self.answer(path)?;
        self.bucket.append(path, more).await
    }

    async fn append_stream<'bucket>(
        &'bucket self,
        path: &Path,
    ) -> StorageResult<Box<dyn AsyncWrite + Send + Unpin + 'bucket>> {
        self.answer(path)?;
        self.bucket.append_stream(path).await
    }

    async fn canonicalize(&self, path: &Path) -> StorageResult<PathBuf> {
        self.answer(path)?;
        self.bucket.canonicalize(path).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> StorageResult<u64> {
        self.answer(from)?;
        self.bucket.copy(from, to).await
    }

    async fn create_folder(&self, path: &Path) -> StorageResult<()> {
        self.answer(path)?;
        self.bucket.create_folder(path).await
    }

    async fn delete(&self, path: &Path, condition: &Condition) -> StorageResult<()> {
        self.answer(path)?;
        self.bucket.delete(path, condition).await
    }

    async fn get(&self, path: &Path) -> StorageResult<(Vec<u8>, RevisionToken)> {
        self.answer(path)?;
        self.note(format!("get {}", path.display()));
        self.bucket.get(path).await
    }

    /// A down bucket keeps no folders, as a server's bucket keeps none.
    fn guarantees(&self) -> Guarantees {
        if self.down {
            Guarantees::none()
        } else {
            self.bucket.guarantees()
        }
    }

    async fn head(&self, path: &Path) -> StorageResult<FileMetadata> {
        self.answer(path)?;
        self.note(format!("head {}", path.display()));
        self.bucket.head(path).await
    }

    async fn list(&self, folder: &Path, depth: Depth) -> StorageResult<Vec<WalkEntry>> {
        self.answer(folder)?;
        self.bucket.list(folder, depth).await
    }

    async fn put(
        &self,
        path: &Path,
        bytes: &[u8],
        condition: &Condition,
    ) -> StorageResult<RevisionToken> {
        self.answer(path)?;
        self.bucket.put(path, bytes, condition).await
    }

    async fn read_stream<'bucket>(
        &'bucket self,
        path: &Path,
    ) -> StorageResult<Box<dyn AsyncRead + Send + Unpin + 'bucket>> {
        self.answer(path)?;
        self.bucket.read_stream(path).await
    }

    async fn remove_folder(&self, path: &Path) -> StorageResult<()> {
        self.answer(path)?;
        self.bucket.remove_folder(path).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> StorageResult<()> {
        self.answer(from)?;
        self.bucket.rename(from, to).await
    }

    async fn walk(
        &self,
        folder: &Path,
        follow_links: bool,
        hidden: bool,
    ) -> StorageResult<Vec<WalkEntry>> {
        self.answer(folder)?;
        self.bucket.walk(folder, follow_links, hidden).await
    }

    async fn write_stream<'bucket>(
        &'bucket self,
        path: &Path,
        condition: Condition,
    ) -> StorageResult<Box<dyn AsyncWrite + Send + Unpin + 'bucket>> {
        self.answer(path)?;
        self.bucket.write_stream(path, condition).await
    }
}

#[cfg(any(feature = "minio", feature = "s3"))]
impl EnsureBucket for TestBucket {
    fn ensure_bucket(
        &self,
        kms_key_arn: Option<&str>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        if self.down {
            return future::ready(Err(StoreError::Backend("no answer".to_owned())));
        }
        self.note(format!("ensure {kms_key_arn:?}"));
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
