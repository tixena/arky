//! Store strings: one text per store, read by the backend its scheme names.
//!
//! A store string is shaped like a URL, such as
//! `s3://acme-notes/vault?region=eu-west-1`. [`Backends`] holds the backends a
//! program can open: the built-in ones, and any of the program's own added
//! with [`Backends::with`]. A text whose scheme no backend answers is a plain
//! path of the file store, so `notes:report.md` names a folder of that name.
//!
//! | Backend | Schemes | Form | Options (default) |
//! | --- | --- | --- | --- |
//! | file | `file`, and every plain path | `file:///srv/notes` absolute; `file:notes`, `notes` relative to the base folder | none |
//! | `MinIO`, with the feature `minio` | `minio+http`, `minio+https` | `minio+https://[key:secret@]host[:port]/bucket[/prefix]` | `region` (`us-east-1`) |
//! | AWS S3, with the feature `s3` | `s3` | `s3://[key:secret@]bucket[/prefix]` | `region` (required), `kms` (none) |
//! | `MongoDB`, with the feature `mongodb` | `mongodb`, `mongodb+srv` | `mongodb[+srv]://[user:password@]host[:port][,host[:port]…]/<database>/<collection>[/<folder>]` | `files` (required): the `MinIO` store of its files, a store string percent-encoded or `arky://<name>`; every other option goes to the driver |
//! | saved store, added with [`Backends::with`] | `arky` | `arky://<name>[/<folder>]` | none |
//!
//! A bucket store string with no keys reads them from `AWS_ACCESS_KEY_ID` and
//! `AWS_SECRET_ACCESS_KEY`. A `MongoDB` collection keeps its files under
//! `<database>/<collection>` of its `files` store. Before a backend reads the
//! text, each `${NAME}` in it is replaced by the variable `NAME`:
//! percent-encoded in a store string, as it is in a plain path. `$$` writes
//! `$`. An error names the store string as written, its variables unexpanded,
//! and never a value read from the environment.

pub mod arky;
mod expand;
mod file;
#[cfg(feature = "minio")]
mod minio;
#[cfg(feature = "mongodb")]
mod mongodb;
#[cfg(feature = "s3")]
mod s3;

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use core::fmt;
use std::path::{Path, PathBuf};

use os_shim::System;
use percent_encoding::percent_decode_str;
#[cfg(any(feature = "minio", feature = "s3"))]
use url::Url;

use crate::lock::LockSettings;
use crate::{
    BoxFuture, Capabilities, DynStore, Precondition, Revision, StoreError, StorePath, Stored,
};
use expand::expand;

/// What a backend opens a store with.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Context<'store> {
    /// The registry, to open a store string nested in an option.
    pub backends: &'store Backends,
    /// The folder a relative `file:` path or plain path is relative to.
    pub base: &'store Path,
    /// The lock the file store and the bucket stores hold during a save.
    pub lock: LockSettings,
    /// The environment, the clock and the files.
    pub system: &'store dyn System,
}

impl fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("backends", self.backends)
            .field("base", &self.base)
            .field("lock", &self.lock)
            .finish_non_exhaustive()
    }
}

/// A kind of store a store string can name.
///
/// A program adds a backend of its own with [`Backends::with`]:
///
/// ```rust
/// use std::path::{Path, PathBuf};
///
/// use arky_store::backend::{Backend, Backends, Context};
/// use arky_store::file::FileStore;
/// use arky_store::lock::LockSettings;
/// use arky_store::{BoxFuture, DynStore, Precondition, StoreError, StorePath};
/// use os_shim::System as _;
/// use os_shim::mock::MemorySystem;
///
/// /// `scratch://<name>`: a file store in `/scratch/<name>`.
/// struct Scratch;
///
/// impl Backend for Scratch {
///     fn open<'call, 'store>(
///         &'store self,
///         text: &'call str,
///         context: Context<'store>,
///     ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
///     where
///         'store: 'call,
///     {
///         Box::pin(async move {
///             let root = PathBuf::from("/scratch").join(text.trim_start_matches("scratch://"));
///             let store = FileStore::new(root, context.system).locking(context.lock);
///             Ok(Box::new(store) as Box<dyn DynStore + 'store>)
///         })
///     }
///
///     fn schemes(&self) -> &'static [&'static str] {
///         &["scratch"]
///     }
///
///     fn split(&self, text: &str) -> Result<(String, String), StoreError> {
///         let rest = text.trim_start_matches("scratch://");
///         let (name, path) = rest.split_once('/').unwrap_or((rest, ""));
///         Ok((format!("scratch://{name}"), path.to_owned()))
///     }
/// }
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let system = MemorySystem::new();
///     let backends = Backends::default().with(Scratch);
///     let lock = LockSettings::default();
///     let store = backends.open("scratch://drafts", Path::new("/"), lock, &system).await?;
///     store.write(&StorePath::new("a.md")?, b"# A\n", &Precondition::Absent).await?;
///     assert_eq!(system.read_to_string(Path::new("/scratch/drafts/a.md"))?, "# A\n");
///     Ok(())
/// }
/// ```
pub trait Backend: Send + Sync {
    /// The store `text` names, its `${…}` variables already replaced. The
    /// backend parses `text` itself.
    ///
    /// The future borrows `text` for the call only; the store it gives may
    /// borrow the backend and `context`.
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidStore` naming the part of `text` that is wrong,
    /// without the text itself: the registry adds it as written. A message
    /// that starts with `text` in backticks, as one naming another store
    /// does, has it written back as written instead.
    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call;

    /// The store string `text` stands for, and its base folder, when this
    /// backend is a name for another store; `None` by default.
    ///
    /// A backend that opens a store string nested in an option follows a
    /// saved name through it, without knowing the configuration.
    fn resolve(&self, _text: &str) -> Option<Result<(String, PathBuf), StoreError>> {
        None
    }

    /// The schemes it answers, such as `["minio+http", "minio+https"]`.
    fn schemes(&self) -> &'static [&'static str];

    /// `text` split into the store string of its container and the path
    /// after it, for a store string written in a path argument:
    /// `s3://acme-notes/vault/a.md` → (`s3://acme-notes`, `vault/a.md`).
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidStore` naming the part of `text` that is wrong,
    /// as in [`Backend::open`].
    fn split(&self, text: &str) -> Result<(String, String), StoreError>;
}

/// The backends a program can open, by scheme.
///
/// [`Backends::default`] holds the built-in backends: `file` always, `MinIO`
/// with the feature `minio`, AWS S3 with the feature `s3`, `MongoDB` with the
/// feature `mongodb`. It leaves out
/// [`arky::SavedBackend`], which needs the saved stores: add it with
/// `with(SavedBackend::new(…))`.
///
/// ```rust
/// use std::path::Path;
///
/// use arky_store::backend::Backends;
/// use arky_store::lock::LockSettings;
/// use arky_store::{Precondition, StoreError, StorePath};
/// use os_shim::System as _;
/// use os_shim::mock::MemorySystem;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let system = MemorySystem::new().with_env("VAULT", "notes")?;
///     let backends = Backends::default();
///     let base = Path::new("/home/me");
///     let lock = LockSettings::default();
///
///     let store = backends.open("file:${VAULT}", base, lock, &system).await?;
///     store.write(&StorePath::new("a.md")?, b"# A\n", &Precondition::Absent).await?;
///     assert_eq!(system.read_to_string(Path::new("/home/me/notes/a.md"))?, "# A\n");
///
///     let (_, path) = backends.open_path("file:notes/a.md", base, lock, &system).await?.ok_or("a plain path")?;
///     assert_eq!(path.as_str(), "notes/a.md");
///     assert!(backends.open_path("notes:report.md", base, lock, &system).await?.is_none());
///
///     let wrong = backends.open("file:notes?x=1", base, lock, &system).await;
///     let refused = StoreError::InvalidStore("`file:notes?x=1`: unknown option x".to_owned());
///     assert_eq!(wrong.err(), Some(refused));
///     Ok(())
/// }
/// ```
pub struct Backends {
    registered: Vec<Box<dyn Backend>>,
}

impl Backends {
    /// The backend that answers `scheme`: the last one registered.
    fn answering(&self, scheme: &str) -> Option<&dyn Backend> {
        self.registered
            .iter()
            .rev()
            .find(|backend| backend.schemes().contains(&scheme))
            .map(AsRef::as_ref)
    }

    /// The backend of the scheme of `text`, or `None` when `text` is a plain
    /// path.
    fn backend_of(&self, text: &str) -> Result<Option<&dyn Backend>, StoreError> {
        let Some((scheme, rest)) = scheme_of(text) else {
            return Ok(None);
        };
        if let Some(backend) = self.answering(scheme) {
            return Ok(Some(backend));
        }
        if rest.starts_with("//") {
            return Err(StoreError::NotConfigured(format!(
                "`{text}`: no backend for {scheme} in this build"
            )));
        }
        Ok(None)
    }

    /// The store `text` names: its variables replaced, then handed to the
    /// backend of its scheme; a text with no registered scheme is a path of
    /// the file backend.
    ///
    /// A relative path is relative to `base`. The file store and the bucket
    /// stores hold `lock` during a save, and every store reads the
    /// environment, the clock and the files through `system`.
    ///
    /// # Errors
    ///
    /// `StoreError::NotConfigured` when a variable `text` names is not set or
    /// not UTF-8, or when `text` is `<scheme>://…` and no backend answers the
    /// scheme. `StoreError::InvalidStore` when `text` does not parse or names
    /// an option its backend does not take. A bucket store string with no
    /// keys is `StoreError::NotConfigured` when `AWS_ACCESS_KEY_ID` or
    /// `AWS_SECRET_ACCESS_KEY` is not set. Every message names `text` as
    /// written, never a variable's value.
    pub async fn open<'store>(
        &'store self,
        text: &str,
        base: &'store Path,
        lock: LockSettings,
        system: &'store dyn System,
    ) -> Result<Box<dyn DynStore + 'store>, StoreError> {
        let context = Context {
            backends: self,
            base,
            lock,
            system,
        };
        let Some(backend) = self.backend_of(text)? else {
            return Ok(file::plain(expand(text, system, false)?.text(), context));
        };
        let expanded = expand(text, system, true)?;
        backend
            .open(expanded.text(), context)
            .await
            .map_err(|error| expanded.as_written(text, expanded.text(), error))
    }

    /// The store a path argument names and the path inside it, or `None` when
    /// `text` has no registered scheme and is a plain path.
    ///
    /// The store is the container of the path: the bucket, or the root
    /// folder, with the rest of `text` as the path inside it.
    ///
    /// # Errors
    ///
    /// The errors of [`Backends::open`], and `StoreError::InvalidStore` when
    /// what follows the container is not a store path.
    pub async fn open_path<'store>(
        &'store self,
        text: &str,
        base: &'store Path,
        lock: LockSettings,
        system: &'store dyn System,
    ) -> Result<Option<(Box<dyn DynStore + 'store>, StorePath)>, StoreError> {
        let Some(backend) = self.backend_of(text)? else {
            return Ok(None);
        };
        let expanded = expand(text, system, true)?;
        let (container, rest) = backend
            .split(expanded.text())
            .map_err(|error| expanded.as_written(text, expanded.text(), error))?;
        let Ok(path) = StorePath::new(&rest) else {
            return Err(StoreError::InvalidStore(format!(
                "`{text}`: what follows the store is not a store path"
            )));
        };
        let context = Context {
            backends: self,
            base,
            lock,
            system,
        };
        let store = backend
            .open(&container, context)
            .await
            .map_err(|error| expanded.as_written(text, &container, error))?;
        Ok(Some((store, path)))
    }

    /// The store string `text` stands for and the folder its relative paths
    /// are relative to: what the backend of its scheme resolves it to, or
    /// `text` and `base` unchanged when that backend names no other store or
    /// no backend answers.
    ///
    /// # Errors
    ///
    /// The errors of the backend's [`Backend::resolve`], such as
    /// `StoreError::NotConfigured` for an `arky://<name>` that no
    /// `arky.toml` saves.
    pub fn resolve(&self, text: &str, base: &Path) -> Result<(String, PathBuf), StoreError> {
        scheme_of(text)
            .and_then(|(scheme, _)| self.answering(scheme))
            .and_then(|backend| backend.resolve(text))
            .unwrap_or_else(|| Ok((text.to_owned(), base.to_path_buf())))
    }

    /// These backends and `backend`; a scheme already answered is answered by
    /// `backend` from now on.
    #[must_use]
    pub fn with<B>(mut self, backend: B) -> Self
    where
        B: Backend + 'static,
    {
        self.registered.push(Box::new(backend));
        self
    }
}

impl fmt::Debug for Backends {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let schemes: Vec<&str> = self
            .registered
            .iter()
            .flat_map(|backend| backend.schemes().iter().copied())
            .collect();
        f.debug_struct("Backends")
            .field("schemes", &schemes)
            .finish_non_exhaustive()
    }
}

impl Default for Backends {
    /// The built-in backends: `file`, `MinIO` with the feature `minio`, AWS
    /// S3 with the feature `s3`, `MongoDB` with the feature `mongodb`.
    fn default() -> Self {
        Self {
            registered: Vec::from([
                boxed(file::FileBackend),
                #[cfg(feature = "minio")]
                boxed(minio::MinioBackend),
                #[cfg(feature = "mongodb")]
                boxed(mongodb::MongoBackend),
                #[cfg(feature = "s3")]
                boxed(s3::S3Backend),
            ]),
        }
    }
}

/// What a bucket store string writes: its URL, its keys and its options.
#[cfg(any(feature = "minio", feature = "s3"))]
struct Bucketed {
    keys: Option<Keys>,
    options: BTreeMap<String, String>,
    url: Url,
}

/// The access key and the secret key a bucket store string carries.
#[cfg(any(feature = "minio", feature = "s3"))]
#[derive(Clone, PartialEq, Eq)]
struct Keys {
    access: String,
    secret: String,
}

#[cfg(any(feature = "minio", feature = "s3"))]
impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keys")
            .field("access", &self.access)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// `store` with `folder` in front of every path it is given, and taken off
/// every path it gives back.
struct Within<'store> {
    folder: StorePath,
    store: Box<dyn DynStore + 'store>,
}

impl Within<'_> {
    /// The path in the store of `path`, a path in the folder.
    fn inside(&self, path: &StorePath) -> Result<StorePath, StoreError> {
        StorePath::new(&format!("{}/{path}", self.folder))
    }

    /// `paths`, paths in the store under the folder, as paths in the folder.
    fn outside(&self, paths: &[StorePath]) -> Result<Vec<StorePath>, StoreError> {
        let start = format!("{}/", self.folder);
        paths
            .iter()
            .map(|path| {
                path.as_str().strip_prefix(&start).map_or_else(
                    || Err(StoreError::InvalidPath(path.as_str().to_owned())),
                    StorePath::new,
                )
            })
            .collect()
    }

    /// The folder, or `prefix` inside it.
    fn under(&self, prefix: Option<&StorePath>) -> Result<StorePath, StoreError> {
        prefix.map_or_else(|| Ok(self.folder.clone()), |inner| self.inside(inner))
    }
}

impl DynStore for Within<'_> {
    fn capabilities(&self) -> Capabilities {
        self.store.capabilities()
    }

    fn delete<'call>(
        &'call self,
        path: &'call StorePath,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<(), StoreError>> {
        Box::pin(async move { self.store.delete(&self.inside(path)?, precondition).await })
    }

    fn ensure_container(&self) -> BoxFuture<'_, Result<(), StoreError>> {
        self.store.ensure_container()
    }

    fn finish_deletions(&self) -> BoxFuture<'_, Result<Vec<StorePath>, StoreError>> {
        self.store.finish_deletions()
    }

    fn list<'call>(
        &'call self,
        prefix: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>> {
        Box::pin(async move {
            let listed = self.store.list(Some(&self.under(prefix)?)).await?;
            self.outside(&listed)
        })
    }

    fn read<'call>(
        &'call self,
        path: &'call StorePath,
    ) -> BoxFuture<'call, Result<Stored, StoreError>> {
        Box::pin(async move { self.store.read(&self.inside(path)?).await })
    }

    fn read_embedded<'call>(
        &'call self,
        document: &'call StorePath,
        file: &'call StorePath,
    ) -> BoxFuture<'call, Result<Vec<u8>, StoreError>> {
        Box::pin(async move {
            self.store
                .read_embedded(&self.inside(document)?, &self.inside(file)?)
                .await
        })
    }

    fn remove_expired_locks<'call>(
        &'call self,
        under: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>> {
        Box::pin(async move {
            let removed = self
                .store
                .remove_expired_locks(Some(&self.under(under)?))
                .await?;
            self.outside(&removed)
        })
    }

    fn write<'call>(
        &'call self,
        path: &'call StorePath,
        bytes: &'call [u8],
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>> {
        Box::pin(async move {
            self.store
                .write(&self.inside(path)?, bytes, precondition)
                .await
        })
    }

    fn write_with_embedded<'call>(
        &'call self,
        path: &'call StorePath,
        text: &'call str,
        files: &'call BTreeMap<StorePath, Vec<u8>>,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>> {
        Box::pin(async move {
            let inside = files
                .iter()
                .map(|(file, bytes)| Ok((self.inside(file)?, bytes.clone())))
                .collect::<Result<BTreeMap<_, _>, StoreError>>()?;
            self.store
                .write_with_embedded(&self.inside(path)?, text, &inside, precondition)
                .await
        })
    }
}

/// `store` with `folder` in front of every path it is given, and taken off
/// every path it gives back.
///
/// It gives a folder inside a store with no prefix of its own, such as a
/// saved store by name. `capabilities`, `ensure_container` and
/// `finish_deletions` pass through unchanged.
///
/// ```rust
/// use std::path::PathBuf;
///
/// use arky_store::backend::within;
/// use arky_store::file::FileStore;
/// use arky_store::{Precondition, StorePath};
/// use os_shim::System as _;
/// use os_shim::mock::MemorySystem;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let system = MemorySystem::new();
///     let store = Box::new(FileStore::new(PathBuf::from("/notes"), &system));
///     let drafts = within(store, StorePath::new("drafts")?);
///     drafts.write(&StorePath::new("a.md")?, b"# A\n", &Precondition::Absent).await?;
///     assert_eq!(system.read_to_string("/notes/drafts/a.md".as_ref())?, "# A\n");
///     assert_eq!(drafts.list(None).await?, [StorePath::new("a.md")?]);
///     Ok(())
/// }
/// ```
#[must_use]
pub fn within<'store>(
    store: Box<dyn DynStore + 'store>,
    folder: StorePath,
) -> Box<dyn DynStore + 'store> {
    Box::new(Within { folder, store })
}

/// `backend` as one of the registry's.
fn boxed<B>(backend: B) -> Box<dyn Backend>
where
    B: Backend + 'static,
{
    Box::new(backend)
}

/// `text` as the URL of a bucket store, its keys and its options. An option
/// not in `takes` and a fragment are refused; an option with no value is
/// left out.
#[cfg(any(feature = "minio", feature = "s3"))]
fn bucket_url(text: &str, takes: &[&str]) -> Result<Bucketed, StoreError> {
    let url = parsed(text)?;
    if url.fragment().is_some() {
        return Err(StoreError::InvalidStore(
            "a fragment, which a store string does not take".to_owned(),
        ));
    }
    let mut options = BTreeMap::new();
    for (option, value) in url.query_pairs() {
        if !takes.contains(&option.as_ref()) {
            return Err(StoreError::InvalidStore(format!("unknown option {option}")));
        }
        if !value.is_empty() {
            options.insert(option.into_owned(), value.into_owned());
        }
    }
    let keys = keys(&url)?;
    Ok(Bucketed { keys, options, url })
}

/// `text` percent-decoded.
fn decoded(text: &str) -> Result<String, StoreError> {
    percent_decode_str(text)
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|error| {
            StoreError::InvalidStore(format!("a part is not UTF-8 once percent-decoded: {error}"))
        })
}

/// The keys `url` carries as `key:secret@`, percent-decoded, or `None` when
/// it carries none.
#[cfg(any(feature = "minio", feature = "s3"))]
fn keys(url: &Url) -> Result<Option<Keys>, StoreError> {
    match (url.username(), url.password()) {
        ("", None) => Ok(None),
        (access, Some(secret)) if !access.is_empty() => Ok(Some(Keys {
            access: decoded(access)?,
            secret: decoded(secret)?,
        })),
        _ => Err(StoreError::InvalidStore(
            "a key with no secret, or a secret with no key: the keys are key:secret@".to_owned(),
        )),
    }
}

/// `error`, from a bucket store opened over the keys of the environment, as
/// `NotConfigured` when a key variable is not set or not UTF-8, its message
/// kept: that is the only `Backend` error opening one gives.
#[cfg(any(feature = "minio", feature = "s3"))]
fn missing_keys(error: StoreError) -> StoreError {
    if let StoreError::Backend(message) = error {
        StoreError::NotConfigured(message)
    } else {
        error
    }
}

/// `text` parsed as a URL.
#[cfg(any(feature = "minio", feature = "s3"))]
fn parsed(text: &str) -> Result<Url, StoreError> {
    Url::parse(text).map_err(|error| StoreError::InvalidStore(error.to_string()))
}

/// The key prefix the path `path` of a URL writes, or `None` for none.
#[cfg(any(feature = "minio", feature = "s3"))]
fn prefix_of(path: &str) -> Result<Option<StorePath>, StoreError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Ok(prefix) = StorePath::new(&decoded(trimmed)?) else {
        return Err(StoreError::InvalidStore(
            "the prefix is not a store path".to_owned(),
        ));
    };
    Ok(Some(prefix))
}

/// The scheme of `text` and what follows its colon, when the text before the
/// first `:` matches `[a-z][a-z0-9+.-]*`.
fn scheme_of(text: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = text.split_once(':')?;
    let mut characters = scheme.chars();
    let starts = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase());
    let follows = characters.all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || "+.-".contains(character)
    });
    (starts && follows).then_some((scheme, rest))
}

#[cfg(test)]
mod tests;
