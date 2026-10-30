//! The store over a file system, through os-shim.
//!
//! [`FileStore`] keeps each file at its store path under a root folder. It
//! reads and writes through an os-shim [`System`]:
//! [`RealSystem`](os_shim::real::RealSystem) for the disk, or
//! [`MemorySystem`](os_shim::mock::MemorySystem) in tests, which needs no disk
//! and moves its own clock when a save waits on a lock.

use alloc::collections::BTreeMap;
use core::fmt::{self, Write as _};
use core::future;
use core::sync::atomic::{AtomicU64, Ordering};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process;

use os_shim::{System, WriteMode};
use sha2::{Digest as _, Sha256};

use crate::lock::{self, Found, LockSettings, Locks};
use crate::{Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored};

/// The count in a temporary name, shared by every store in the process.
static WRITES: AtomicU64 = AtomicU64::new(0);

/// Files under a root folder, read and written through os-shim. A revision
/// is the SHA-256 of the file's bytes.
///
/// The revision's text is `sha256:` and 64 lowercase hex digits. A write goes
/// to a hidden temporary file beside the target, which is then renamed over
/// it, so a reader never sees half a file. A precondition is checked, then
/// the file written, in two steps. Only [`Store::write_with_embedded`] takes
/// the document's lock; a plain write or delete does not.
pub struct FileStore<'system> {
    follow_links: bool,
    hidden: bool,
    lock: LockSettings,
    root: PathBuf,
    system: &'system dyn System,
}

impl<'system> FileStore<'system> {
    /// The bytes of the file at `path`, or `None` when no file is there.
    fn bytes(&self, path: &StorePath) -> Result<Option<Vec<u8>>, StoreError> {
        let (folder, name) = self.place(path);
        let mut reader = match self.system.open(&folder.join(name)) {
            Ok(reader) => reader,
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(backend(path, &error)),
        };
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| backend(path, &error))?;
        Ok(Some(bytes))
    }

    fn check(&self, path: &StorePath, precondition: &Precondition) -> Result<(), StoreError> {
        let holds = match precondition {
            Precondition::Absent => self.bytes(path)?.is_none(),
            Precondition::Matches(expected) => self
                .bytes(path)?
                .is_some_and(|bytes| revision(&bytes) == *expected),
            Precondition::None => true,
        };
        if holds {
            Ok(())
        } else {
            Err(StoreError::Conflict(path.clone()))
        }
    }

    fn delete_file(&self, path: &StorePath, precondition: &Precondition) -> Result<(), StoreError> {
        self.check(path, precondition)?;
        let (folder, name) = self.place(path);
        self.system
            .remove_file(&folder.join(name))
            .map_err(|error| failure(path, &error))
    }

    fn list_files(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let mut paths = self.walk_files(prefix)?;
        paths.retain(|path| !path.as_str().ends_with(lock::SUFFIX));
        Ok(paths)
    }

    /// The two flags `walk_dir` takes, `false` by default.
    ///
    /// They set what [`Store::list`] and `remove_expired_locks` walk:
    /// `follow_links` follows symbolic links, and `hidden` includes entries
    /// under a path part that starts with `.`. Over `RealSystem`, the walk also
    /// leaves out what `.gitignore` files name.
    #[must_use]
    pub fn listing(self, follow_links: bool, hidden: bool) -> Self {
        Self {
            follow_links,
            hidden,
            ..self
        }
    }

    /// The lock at `lock`, its bytes read before its modification time.
    fn lock_found(&self, lock: &StorePath) -> Result<Option<Found>, StoreError> {
        let Some(bytes) = self.bytes(lock)? else {
            return Ok(None);
        };
        let (folder, name) = self.place(lock);
        let taken = match self.system.metadata(&folder.join(name)) {
            Ok(metadata) => metadata.modified,
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(backend(lock, &error)),
        };
        Ok(Some(Found {
            holder: String::from_utf8_lossy(&bytes).into_owned(),
            taken,
            tag: bytes,
        }))
    }

    /// Makes the lock at `lock` holding `guid`, and its folder, only when
    /// nothing is at its path.
    fn lock_made(&self, lock: &StorePath, guid: &str) -> Result<bool, StoreError> {
        let (folder, name) = self.place(lock);
        self.system
            .create_dir_all(&folder)
            .map_err(|error| backend(lock, &error))?;
        match self
            .system
            .write(&folder.join(name), guid.as_bytes(), WriteMode::CreateNew)
        {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(backend(lock, &error)),
        }
    }

    /// Removes the lock at `lock` only while it holds the bytes of `found`.
    fn lock_removed(&self, lock: &StorePath, found: &Found) -> Result<bool, StoreError> {
        let (folder, name) = self.place(lock);
        match self.system.remove_file_if(&folder.join(name), &found.tag) {
            Ok(removed) => Ok(removed),
            Err(error) if missing(&error) => Ok(false),
            Err(error) => Err(backend(lock, &error)),
        }
    }

    /// How long the lock of a save lasts and how long a save waits on another,
    /// `LockSettings::default()` by default.
    ///
    /// The same settings decide which locks `remove_expired_locks` finds
    /// expired.
    #[must_use]
    pub fn locking(self, lock: LockSettings) -> Self {
        Self { lock, ..self }
    }

    /// A store whose listing follows no link and leaves hidden entries out.
    ///
    /// Each store path is relative to `root`: with the root `/notes`, the file
    /// `recipes/bread.md` is `/notes/recipes/bread.md` on `system`.
    ///
    /// ```rust
    /// use std::path::PathBuf;
    ///
    /// use arky_store::file::FileStore;
    /// use arky_store::{Precondition, Store as _, StorePath};
    /// use os_shim::System as _;
    /// use os_shim::mock::MemorySystem;
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let system = MemorySystem::new();
    ///     let store = FileStore::new(PathBuf::from("/notes"), &system);
    ///     let path = StorePath::new("recipes/bread.md")?;
    ///     store.write(&path, b"# Bread\n", &Precondition::Absent).await?;
    ///     let on_system = system.read_to_string("/notes/recipes/bread.md".as_ref())?;
    ///     assert_eq!(on_system, "# Bread\n");
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn new(root: PathBuf, system: &'system dyn System) -> Self {
        Self {
            follow_links: false,
            hidden: false,
            lock: LockSettings::default(),
            root,
            system,
        }
    }

    /// The folder that holds the file at `path`, and the file's name.
    fn place<'path>(&self, path: &'path StorePath) -> (PathBuf, &'path str) {
        let (folders, name) = path
            .as_str()
            .rsplit_once('/')
            .unwrap_or(("", path.as_str()));
        let mut folder = self.root.clone();
        folder.extend(folders.split_terminator('/'));
        (folder, name)
    }

    /// The bytes of the file at `file`, or `NotFound` when that path holds no
    /// file, a folder included.
    fn read_embedded_file(&self, file: &StorePath) -> Result<Vec<u8>, StoreError> {
        let (folder, name) = self.place(file);
        let held = self
            .system
            .is_file(&folder.join(name))
            .map_err(|error| backend(file, &error))?;
        if held {
            Ok(self.read_file(file)?.bytes)
        } else {
            Err(StoreError::NotFound(file.clone()))
        }
    }

    fn read_file(&self, path: &StorePath) -> Result<Stored, StoreError> {
        let bytes = self
            .bytes(path)?
            .ok_or_else(|| StoreError::NotFound(path.clone()))?;
        let revision = revision(&bytes);
        Ok(Stored { bytes, revision })
    }

    /// Removes each expired lock under `under` (the whole store when `None`),
    /// each one only while it is still the lock found, and gives the path of
    /// each lock it removed, sorted.
    ///
    /// It clears the locks left behind by saves that stopped. Only files whose
    /// names end with [`lock::SUFFIX`] count as locks, and a lock has expired
    /// once it is older than the store's [`LockSettings::expires`].
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` when the store cannot be listed or a lock cannot
    /// be read or removed, and `StoreError::InvalidPath` for a name under
    /// `under` that is not a store path.
    pub async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        let mut locks = self.walk_files(under)?;
        locks.retain(|path| path.as_str().ends_with(lock::SUFFIX));
        lock::remove_expired(self, &locks, self.lock).await
    }

    /// The store path of `file`, a file `walk_dir` found under the root.
    fn store_path(&self, file: &Path) -> Result<StorePath, StoreError> {
        let relative = file
            .strip_prefix(&self.root)
            .map_err(|error| StoreError::Backend(format!("{}: {error}", file.display())))?;
        let text = relative
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| StoreError::InvalidPath(relative.to_string_lossy().into_owned()))?
            .join("/");
        StorePath::new(&text)
    }

    /// A name beside the target, unique to this write in this process.
    fn temporary(folder: &Path, name: &str) -> PathBuf {
        let count = WRITES.fetch_add(1, Ordering::Relaxed);
        folder.join(format!(".{name}.arky-write-{}-{count}", process::id()))
    }

    /// The path of each file under `prefix`, locks included, sorted.
    fn walk_files(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let mut folder = self.root.clone();
        folder.extend(prefix.map_or("", StorePath::as_str).split_terminator('/'));
        let entries = self
            .system
            .walk_dir(&folder, self.follow_links, self.hidden)
            .map_err(|error| StoreError::Backend(format!("{}: {error}", folder.display())))?;
        let mut paths = entries
            .iter()
            .filter(|entry| entry.is_file)
            .map(|entry| self.store_path(&entry.path))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort_unstable();
        Ok(paths)
    }

    fn write_file(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        self.check(path, precondition)?;
        let (folder, name) = self.place(path);
        self.system
            .create_dir_all(&folder)
            .map_err(|error| backend(path, &error))?;
        let temporary = Self::temporary(&folder, name);
        let written = self
            .system
            .write(&temporary, bytes, WriteMode::Replace)
            .and_then(|()| self.system.rename(&temporary, &folder.join(name)));
        if let Err(error) = written {
            let _: io::Result<()> = self.system.remove_file(&temporary);
            return Err(backend(path, &error));
        }
        Ok(revision(bytes))
    }
}

impl Locks for FileStore<'_> {
    fn create(
        &self,
        lock: &StorePath,
        guid: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        future::ready(self.lock_made(lock, guid))
    }

    fn find(
        &self,
        lock: &StorePath,
    ) -> impl Future<Output = Result<Option<Found>, StoreError>> + Send {
        future::ready(self.lock_found(lock))
    }

    fn remove_if(
        &self,
        lock: &StorePath,
        found: &Found,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        future::ready(self.lock_removed(lock, found))
    }

    fn revision(
        &self,
        path: &StorePath,
    ) -> impl Future<Output = Result<Option<Revision>, StoreError>> + Send {
        future::ready(
            self.bytes(path)
                .map(|held| held.map(|bytes| revision(&bytes))),
        )
    }

    fn system(&self) -> &dyn System {
        self.system
    }
}

/// os-shim is synchronous, so each operation is done when it is called and its
/// future is ready.
impl Store for FileStore<'_> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            atomic_preconditions: false,
        }
    }

    /// Deletes the file at `path` when `precondition` holds. Where no file is,
    /// it fails with `StoreError::NotFound` under `Precondition::None` and
    /// `Precondition::Absent`.
    fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        future::ready(self.delete_file(path, precondition))
    }

    /// Every file under `prefix`, sorted, with lock files left out, and hidden
    /// entries left out unless [`FileStore::listing`] lets them in.
    fn list(
        &self,
        prefix: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        future::ready(self.list_files(prefix))
    }

    fn read(&self, path: &StorePath) -> impl Future<Output = Result<Stored, StoreError>> + Send {
        future::ready(self.read_file(path))
    }

    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send {
        future::ready(self.read_embedded_file(file))
    }

    fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        future::ready(self.write_file(path, bytes, precondition))
    }

    /// Holds the lock of `path` from before the first file to after the
    /// document.
    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        lock::write_with_embedded(self, path, text, files, precondition, self.lock)
    }
}

fn backend(path: &StorePath, error: &io::Error) -> StoreError {
    StoreError::Backend(format!("{path}: {error}"))
}

/// `NotFound` for a missing file, `Backend` for any other failure.
fn failure(path: &StorePath, error: &io::Error) -> StoreError {
    if missing(error) {
        StoreError::NotFound(path.clone())
    } else {
        backend(path, error)
    }
}

/// Whether `error` says no file is at the path: nothing there, or a file
/// where a folder of the path should be.
fn missing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// `sha256:` and the 64 lowercase hexadecimal digits of the hash of `bytes`.
pub(crate) fn revision(bytes: &[u8]) -> Revision {
    Revision(format!("sha256:{}", sha256(bytes)))
}

/// The 64 lowercase hexadecimal digits of the SHA-256 of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _: fmt::Result = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests;
