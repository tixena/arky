//! Keeps arky's markdown documents, and the files they embed, in a folder, in
//! an S3-compatible bucket or in `MongoDB`. Every store implements one async
//! trait, [`Store`], and the [`document`] functions save and load a whole
//! document through any of them.
//!
//! # The problem and who it is for
//!
//! A program that edits markdown notes has to save them somewhere, keep each
//! picture a note embeds next to it, and never overwrite a change another
//! process made in between. arky-store does that work. Each read gives a
//! [`Revision`] and each write takes a [`Precondition`], so a save made from a
//! stale read fails with [`StoreError::Conflict`]. In the file store and the
//! bucket stores (`MinIO` and AWS S3), a save of a note and its pictures holds
//! a lock that expires, and these stores also run over os-shim's in-memory
//! twins, so their tests need no disk, no server and no real clock.
//!
//! Use it beside [`arky`], which parses and writes the documents, when your
//! program keeps markdown documents and wants one interface over a folder, a
//! bucket and a database.
//!
//! # The stores
//!
//! | Store | What it keeps | Feature |
//! | --- | --- | --- |
//! | [`FileStore`](file::FileStore) | Each file at its path under a root folder, through an os-shim `System`: the disk, or `MemorySystem` in tests. | none |
#![cfg_attr(
    any(feature = "minio", feature = "s3"),
    doc = "| [`ObjectStore`](object::ObjectStore) | Each file as an object of one bucket, under an optional key prefix: a `MinIO` or other S3-compatible server through `MinioBucket` with `minio`, an AWS S3 bucket through `S3Bucket` with `s3`, or `MemoryBucket` in tests. The bucket stores are this store over `MinioBucket` and over `S3Bucket`. `ensure` creates a missing bucket. | `minio` or `s3` |"
)]
#![cfg_attr(
    not(any(feature = "minio", feature = "s3")),
    doc = "| `ObjectStore` | Each file as an object of one bucket, under an optional key prefix: a `MinIO` or other S3-compatible server through `MinioBucket` with `minio`, an AWS S3 bucket through `S3Bucket` with `s3`, or `MemoryBucket` in tests. The bucket stores are this store over `MinioBucket` and over `S3Bucket`. `ensure` creates a missing bucket. | `minio` or `s3` |"
)]
#![cfg_attr(
    feature = "mongodb",
    doc = "| [`MongoStore`](mongo::MongoStore) | Each document as one `MongoDB` record whose frontmatter a query can name, and each embedded file once, by its SHA-256, in a `MinIO` bucket. `ensure_files_bucket` creates a missing bucket of its files. | `mongodb`, which turns on `minio` |"
)]
#![cfg_attr(
    not(feature = "mongodb"),
    doc = "| `MongoStore` | Each document as one `MongoDB` record whose frontmatter a query can name, and each embedded file once, by its SHA-256, in a `MinIO` bucket. `ensure_files_bucket` creates a missing bucket of its files. | `mongodb`, which turns on `minio` |"
)]
//! | [`AnyStore`](any::AnyStore) | Whichever of the three a program opened at run time. | none; each variant needs its store's feature |
//!
//! # Getting started
//!
//! Add arky-store and arky, arky for the forms and
//! the write options. Add os-shim for a `System`, and tokio to run the
//! futures:
//!
//! ```toml
//! [dependencies]
//! arky = "0.2"
//! arky-store = "0.2"
//! os-shim = "0.2"
//! tokio = { version = "1", features = ["rt", "macros"] }
//! ```
//!
//! This saves a recipe into a file store over `MemorySystem`, reads the stored
//! text back, and loads the document again. A store keeps arky's form of a
//! document, so the TOML frontmatter is written as YAML and the `*` list as a
//! `-` list.
//!
//! ```rust
//! use std::path::PathBuf;
//!
//! use arky::bundle::Missing;
//! use arky::forms::Form;
//! use arky::writer::{WriteOptions, write};
//! use arky_store::file::FileStore;
//! use arky_store::{Precondition, Store as _, StorePath, document};
//! use os_shim::mock::MemorySystem;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let system = MemorySystem::new();
//!     let store = FileStore::new(PathBuf::from("/kitchen"), &system);
//!     let path = StorePath::new("recipes/bread.md")?;
//!     let options = WriteOptions::default();
//!
//!     let given = "+++\ntitle = \"Bread\"\n+++\n# Bread\n\n* flour\n* water\n";
//!     document::put(
//!         &store,
//!         &path,
//!         Form::Markdown,
//!         given.as_bytes(),
//!         &options,
//!         &Precondition::Absent,
//!         Missing::Refuse,
//!     )
//!     .await?;
//!
//!     let kept = "---\ntitle: Bread\n---\n# Bread\n\n- flour\n- water\n";
//!     assert_eq!(store.read(&path).await?.bytes, kept.as_bytes());
//!
//!     let bundle = document::get(&store, &path, Missing::Refuse).await?;
//!     assert_eq!(write(bundle.document(), &options)?, kept);
//!     Ok(())
//! }
//! ```
//!
//! # Use cases
//!
//! ## Save without losing another process's change
//!
//! An editor keeps the [`Revision`] of what it read and saves with
//! [`Precondition::Matches`]. When another process saved in between, the save
//! fails with [`StoreError::Conflict`] and the other change stays.
//! [`Precondition::Absent`] creates a file only where none is. In the file
//! store, a revision is `sha256:` and the hex SHA-256 of the bytes.
//!
//! ```rust
//! use std::path::PathBuf;
//!
//! use arky_store::file::FileStore;
//! use arky_store::{Precondition, Store as _, StoreError, StorePath};
//! use os_shim::mock::MemorySystem;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let system = MemorySystem::new();
//!     let store = FileStore::new(PathBuf::from("/notes"), &system);
//!     let path = StorePath::new("shopping.md")?;
//!
//!     let created = store.write(&path, b"- buy flour\n", &Precondition::Absent).await?;
//!     assert_eq!(
//!         created.0,
//!         "sha256:b0bfa393f05d0de4aa77bc224fe78f44c17578dda48c0482267b60a20bd975e9",
//!     );
//!
//!     let read = store.read(&path).await?;
//!     store.write(&path, b"- buy flour\n- buy salt\n", &Precondition::None).await?;
//!
//!     let stale = Precondition::Matches(read.revision);
//!     let saved = store.write(&path, b"- buy yeast\n", &stale).await;
//!     assert_eq!(saved, Err(StoreError::Conflict(path.clone())));
//!     assert_eq!(store.read(&path).await?.bytes, b"- buy flour\n- buy salt\n");
//!     Ok(())
//! }
//! ```
//!
//! ## A document and the pictures it embeds
//!
//! [`document::put_bundle`] stores a document and each file it embeds at its
//! path from the document's folder, and [`document::get`] gives them back as
//! one [`BundledDocument`](arky::bundle::BundledDocument). An embedded note's
//! own embeds travel too. [`document::put`] does the same from bytes in any
//! [`Form`](arky::forms::Form), such as markdown whose pictures are data URIs,
//! or a zip.
//!
//! ```rust
//! use std::collections::BTreeMap;
//! use std::path::PathBuf;
//!
//! use arky::bundle::{BundledDocument, Missing};
//! use arky::parser::parse;
//! use arky::writer::WriteOptions;
//! use arky_store::file::FileStore;
//! use arky_store::{Precondition, Store as _, StorePath, document};
//! use os_shim::mock::MemorySystem;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let system = MemorySystem::new();
//!     let store = FileStore::new(PathBuf::from("/blog"), &system);
//!     let path = StorePath::new("posts/bread.md")?;
//!
//!     let text = "# Bread\n\n![The loaf](media/loaf.png)\n";
//!     let picture = b"\x89PNG\r\n\x1a\n not a real picture".to_vec();
//!     let files = BTreeMap::from([("media/loaf.png".to_owned(), picture.clone())]);
//!     let bundle = BundledDocument::new(parse(text).document, files, Missing::Refuse)?;
//!
//!     let options = WriteOptions::default();
//!     document::put_bundle(&store, &path, &bundle, &options, &Precondition::Absent).await?;
//!     let stored: Vec<String> = store.list(None).await?.iter().map(ToString::to_string).collect();
//!     assert_eq!(stored, ["posts/bread.md", "posts/media/loaf.png"]);
//!
//!     let back = document::get(&store, &path, Missing::Refuse).await?;
//!     assert_eq!(back.files(), ["media/loaf.png"]);
//!     assert_eq!(back.file("media/loaf.png"), Some(picture.as_slice()));
//!     assert_eq!(back, bundle);
//!     Ok(())
//! }
//! ```
//!
//! ## Locks that expire
//!
//! A save of a document and its files holds the lock `<path>.arky-lock` beside
//! the document. A save that finds a lock that has not expired waits once (2
//! seconds by default) and looks again. A lock still there and not expired
//! then fails the save with [`StoreError::Conflict`]. A lock left
//! by a save that crashed expires (after 5 seconds by default): the next save
//! removes it, and `remove_expired_locks` sweeps every one. Over
//! `MemorySystem`, the wait moves the system's clock and returns at once. See
//! [`lock`].
//!
//! ```rust
//! use std::path::PathBuf;
//! use std::time::Duration;
//!
//! use arky::bundle::Missing;
//! use arky::forms::Form;
//! use arky::writer::WriteOptions;
//! use arky_store::document::{self, DocumentError};
//! use arky_store::file::FileStore;
//! use arky_store::lock::lock_of;
//! use arky_store::{Precondition, Store as _, StoreError, StorePath};
//! use os_shim::System as _;
//! use os_shim::mock::MemorySystem;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let system = MemorySystem::new().with_file("/notes/report.md.arky-lock", b"crashed")?;
//!     let store = FileStore::new(PathBuf::from("/notes"), &system);
//!     let path = StorePath::new("report.md")?;
//!     let options = WriteOptions::default();
//!     let started = system.now();
//!
//!     let refused = document::put(
//!         &store,
//!         &path,
//!         Form::Markdown,
//!         b"# Report\n",
//!         &options,
//!         &Precondition::None,
//!         Missing::Refuse,
//!     )
//!     .await;
//!     assert!(matches!(refused, Err(DocumentError::Store(StoreError::Conflict(_)))));
//!     assert_eq!(system.now().duration_since(started)?, Duration::from_secs(2));
//!
//!     system.advance(Duration::from_secs(4))?;
//!     assert_eq!(store.remove_expired_locks(None).await?, [lock_of(&path)?]);
//!
//!     document::put(
//!         &store,
//!         &path,
//!         Form::Markdown,
//!         b"# Report\n",
//!         &options,
//!         &Precondition::None,
//!         Missing::Refuse,
//!     )
//!     .await?;
//!     assert_eq!(store.read(&path).await?.bytes, b"# Report\n");
//!     Ok(())
//! }
//! ```
#![cfg_attr(
    any(feature = "minio", feature = "s3"),
    doc = r##"
## A bucket, and the same bucket in memory in tests

[`ObjectStore`](object::ObjectStore) keeps each file as an object under a key
prefix: the file `posts/hello.md` under the prefix `blog` is the object
`blog/posts/hello.md`. A precondition goes to the bucket with the write, so
the check and the write are one step. In production, `ObjectStore::open`
builds the store over a `MinioBucket` from an `ObjectConfig`, with its keys
read from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. For an AWS S3
bucket, `ObjectStore::open_s3` builds the store over an `S3Bucket` from an
`S3ObjectConfig`, with the same two variables. In a test,
[`ObjectStore::new`](object::ObjectStore::new) takes os-shim's `MemoryBucket`,
which reaches no server.

```rust
use arky::bundle::Missing;
use arky::forms::Form;
use arky::writer::WriteOptions;
use arky_store::object::ObjectStore;
use arky_store::{Precondition, Store as _, StoreError, StorePath, document};
use os_shim::bucket::{Bucket as _, MemoryBucket};
use os_shim::mock::MemorySystem;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::new();
    let bucket = MemoryBucket::default();
    let store = ObjectStore::new(bucket.clone(), Some(StorePath::new("blog")?), &system);
    let path = StorePath::new("posts/hello.md")?;

    document::put(
        &store,
        &path,
        Form::Markdown,
        b"# Hello\n",
        &WriteOptions::default(),
        &Precondition::Absent,
        Missing::Refuse,
    )
    .await?;
    let (bytes, _revision) = bucket.get("/blog/posts/hello.md".as_ref()).await?;
    assert_eq!(bytes, b"# Hello\n");

    let again = store.write(&path, b"# Hello again\n", &Precondition::Absent).await;
    assert_eq!(again, Err(StoreError::Conflict(path.clone())));
    Ok(())
}
```
"##
)]
#![cfg_attr(
    feature = "mongodb",
    doc = r#"
## Find notes by their frontmatter in `MongoDB`

[`MongoStore`](mongo::MongoStore) keeps each document as a record whose
frontmatter is plain BSON under `doc.data`, dates as BSON dates, so a query
can name a frontmatter key. [`MongoStore::find`](mongo::MongoStore::find)
takes such a filter, and [`frontmatter_is`](mongo::record::frontmatter_is)
builds one for a key. A `MongoStore` needs a running server, but the
[`record`](mongo::record) functions convert a record with none, which is how a
test checks a filter or a record. A found record's `document` written with
[`arky::writer::write`] gives the document's text in arky's form.

```rust
use std::collections::BTreeMap;

use arky::parser::ast::Value;
use arky::parser::parse;
use arky::writer::{WriteOptions, write};
use arky_store::mongo::record::{from_record, frontmatter_is, revision_of, to_record};
use bson::oid::ObjectId;
use bson::{DateTime, doc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = "---\nstatus: open\ndue: 2026-11-02\n---\n# Fix the oven\n";
    let stored = from_record(doc! {
        "_id": ObjectId::parse_str("6ad42f681472f05ab2ed9be7")?,
        "doc": bson::serialize_to_bson(&parse(text).document)?,
        "path": "tasks/oven.md",
        "revision": revision_of(text, &BTreeMap::new()).0,
    })?;
    assert_eq!(stored.path, "tasks/oven.md");
    assert_eq!(
        write(&stored.document, &WriteOptions::default())?,
        "---\nstatus: open\ndue: !!timestamp 2026-11-02\n---\n# Fix the oven\n",
    );

    let record = to_record(&stored)?;
    let data = record.get_document("doc")?.get_document("data")?;
    assert_eq!(data.get_str("status")?, "open");
    let due = DateTime::parse_rfc3339_str("2026-11-02T00:00:00Z")?;
    assert_eq!(data.get_datetime("due")?, &due);

    let open = frontmatter_is("status", &Value::String("open".to_owned()))?;
    assert_eq!(open, doc! { "doc.data.status": "open" });
    Ok(())
}
```
"#
)]
//!
//! # Features
//!
//! No feature is on by default.
//!
//! | Feature | What it turns on | What your manifest also needs |
//! | --- | --- | --- |
//! | `minio` | The `object` module, `AnyStore::Minio`, and the `minio+http` and `minio+https` store strings of the `backend` module. | `os-shim` with its `minio` feature, to name `MinioBucket` or `MemoryBucket`. |
//! | `s3` | The `object` module, `AnyStore::S3`, and the `s3` store strings of the `backend` module. | `os-shim` with its `s3` feature, to name `S3Bucket`. |
//! | `mongodb` | The `mongo` module, `AnyStore::Mongo`, and `minio`. | `bson = { version = "3.1", features = ["serde"] }`, to write the filters `MongoStore::find` takes. A `MongoDB` server, and an S3-compatible server for the embedded files. |
//! | `binary-format` | arky's `binary-format`, so `document::put` takes `Form::Binary`. | Nothing more. |
//!
//! ```toml
//! [dependencies]
//! arky-store = { version = "0.2", features = ["mongodb"] }
//! bson = { version = "3.1", features = ["serde"] }
//! os-shim = { version = "0.2", features = ["minio"] }
//! ```
//!
//! Every method of [`Store`] returns a future. Over `MemorySystem` and
//! `MemoryBucket` any executor runs them, and they never wait on a real clock.
//! Over the disk, a bucket on a server or `MongoDB`, run them on tokio with its
//! timer on, as `#[tokio::main]` builds it: a save that finds another save's
//! lock, and a `MongoDB` save that retries a claim, wait on tokio's timer.
//!
//! # Limits
//!
//! - A store keeps arky's form of what [`document::put`] gives it, not the
//!   bytes: a round trip is not byte-exact. [`Store::write`] on the file store
//!   and the bucket stores keeps the bytes as given.
//! - The `MongoDB` store keeps the parsed tree and reads a document back in
//!   arky's form: a valid frontmatter in YAML, whatever syntax it was written
//!   in, and a frontmatter no entry of reads as it was written. A record nests
//!   at most 100 levels, so a document of 13 nested list items is refused.
//! - Each store writes its own [`Revision`]: compare revisions of one store
//!   only.
//! - The file store checks a precondition and then writes, in two steps. In
//!   every store, a plain [`Store::write`] or [`Store::delete`] takes no lock.
//! - A save that runs past its lock's expiry fails with
//!   [`StoreError::Conflict`]. Raise [`LockSettings::expires`](lock::LockSettings::expires)
//!   for saves of many large files.
//! - What a delete where no file is answers differs per store: see each
//!   store's `delete`.
//! - There is no in-memory `MongoDB` store. `AnyStore::Minio` holds only a
//!   store over `MinioBucket`, and `AnyStore::S3` only one over `S3Bucket`.
//! - An object key holds at most 1024 bytes, the key prefix included.

extern crate alloc;

#[cfg(feature = "mongodb")]
tixschema::decode_with_foundation!();
#[cfg(feature = "mongodb")]
tixschema::mongodb_foundation!();

pub mod any;
pub mod backend;
pub mod config;
pub mod document;
pub mod file;
pub mod lock;
#[cfg(feature = "mongodb")]
pub mod mongo;
#[cfg(any(feature = "minio", feature = "s3"))]
pub mod object;

use alloc::collections::BTreeMap;
use core::pin::Pin;
use core::{fmt, future};

/// A file's place in a store: parts joined by `/`, none empty, none `.` or
/// `..`, no `\`, with no `/` at either end.
///
/// A store path is relative to the store: to the root folder of the file
/// store, to the key prefix of a bucket store. Paths sort in byte order,
/// the order [`Store::list`] gives them in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorePath(String);

impl StorePath {
    /// The path as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path of `name`, a path inside a bundle, from the folder of `self`:
    /// `notes/report.md` and `media/logo.png` give `notes/media/logo.png`.
    ///
    /// This is where a store keeps a file a document embeds.
    ///
    /// ```rust
    /// use arky_store::{StoreError, StorePath};
    ///
    /// let report = StorePath::new("notes/report.md")?;
    /// assert_eq!(report.beside("media/logo.png")?.as_str(), "notes/media/logo.png");
    ///
    /// let refused = Err(StoreError::InvalidPath("notes/../logo.png".to_owned()));
    /// assert_eq!(report.beside("../logo.png"), refused);
    /// # Ok::<(), StoreError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidPath` when the path it gives breaks one of the
    /// rules of [`StorePath`].
    pub fn beside(&self, name: &str) -> Result<Self, StoreError> {
        self.0.rsplit_once('/').map_or_else(
            || Self::new(name),
            |(folder, _)| Self::new(&format!("{folder}/{name}")),
        )
    }

    /// The store path `text` names.
    ///
    /// ```rust
    /// use arky_store::{StoreError, StorePath};
    ///
    /// assert_eq!(StorePath::new("notes/report.md")?.as_str(), "notes/report.md");
    /// for broken in ["", "/notes/a.md", "notes/", "notes//a.md", "./a.md", "notes/../a.md", "a\\b.md"] {
    ///     assert_eq!(StorePath::new(broken), Err(StoreError::InvalidPath(broken.to_owned())));
    /// }
    /// # Ok::<(), StoreError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidPath` when `text` breaks one of the rules of
    /// [`StorePath`].
    pub fn new(text: &str) -> Result<Self, StoreError> {
        let valid = text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains('\\'));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(StoreError::InvalidPath(text.to_owned()))
        }
    }
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a store gives with the bytes it read, and takes back for a write that
/// waits for nothing else to have changed the file.
///
/// Its text is the store's own: `sha256:` and the hex SHA-256 of the bytes in
/// the file store, the object's `ETag` as the bucket gives it in the bucket
/// stores, and the hash of the document's text and its files in the `MongoDB`
/// store. Compare revisions of one store only. A store builds one with
/// [`Revision::new`]; a caller gets one from [`Store::read`] or
/// [`Store::write`] and passes it back in [`Precondition::Matches`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Revision(pub String);

impl Revision {
    /// A revision holding the store's own text, such as an entity tag or a
    /// hash. Compare revisions of one store only.
    #[must_use]
    pub fn new<T>(text: T) -> Self
    where
        T: Into<String>,
    {
        Self(text.into())
    }
}

/// A file as [`Store::read`] gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stored {
    /// The file's bytes. The `MongoDB` store gives a document in arky's form.
    pub bytes: Vec<u8>,
    /// The revision to pass back in [`Precondition::Matches`].
    pub revision: Revision,
}

impl Stored {
    /// A file's bytes and their revision, as [`Store::read`] gives them.
    #[must_use]
    pub const fn new(bytes: Vec<u8>, revision: Revision) -> Self {
        Self { bytes, revision }
    }
}

/// What must hold for a write or a delete to go ahead.
///
/// A precondition that does not hold fails the write or the delete with
/// [`StoreError::Conflict`], and the file stays as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precondition {
    /// No file is at the path.
    Absent,
    /// The file at the path still has this revision.
    Matches(Revision),
    /// Nothing: the write replaces whatever is there.
    None,
}

/// Why a store refused or failed an operation.
///
/// The messages of `Conflict`, `InvalidPath` and `NotFound` name the path:
/// `notes/none.md: no file at this path`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The store failed: the file system, the network, the server.
    #[error("{0}")]
    Backend(String),
    /// The precondition does not hold, or another save holds the document's
    /// lock.
    #[error("{0}: the file changed, or a file is already there")]
    Conflict(StorePath),
    /// Text that is not a store path, or a path the store cannot hold, such as
    /// an object key longer than 1024 bytes.
    #[error("`{0}` is not a store path")]
    InvalidPath(String),
    /// A store string that does not parse, or that names an option or a part
    /// its backend does not take: `` `s3://acme?regoin=eu-west-1`: unknown option regoin ``.
    #[error("{0}")]
    InvalidStore(String),
    /// A store that cannot be opened as configured: a variable it names is not
    /// set or not UTF-8, its backend is not built in, a saved store it names
    /// is not defined, or an arky.toml that does not read.
    #[error("{0}")]
    NotConfigured(String),
    /// No file is at the path.
    #[error("{0}: no file at this path")]
    NotFound(StorePath),
    /// An operation this store does not have, such as `remove_expired_locks`
    /// on the `MongoDB` store.
    #[error("this store cannot {0}")]
    Unsupported(&'static str),
}

/// What a store can do that not every store can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// A precondition is checked and the write done as one step: `false` in
    /// the file store, `true` in the `MinIO` store, the AWS S3 store and the
    /// `MongoDB` store.
    pub atomic_preconditions: bool,
}

impl Capabilities {
    /// What a store can do: whether it checks a precondition and writes in
    /// one step.
    #[must_use]
    pub const fn new(atomic_preconditions: bool) -> Self {
        Self {
            atomic_preconditions,
        }
    }
}

/// Where documents and the files they embed are kept: files read, written,
/// deleted and listed by [`StorePath`].
///
/// Each method returns a future that is `Send`. [`file::FileStore`] is always
/// here, `object::ObjectStore` with the `minio` feature or the `s3` feature,
/// `mongo::MongoStore` with the `mongodb` feature, and [`any::AnyStore`] holds
/// any of them. A type outside this crate implements `Store` with
/// [`Revision::new`], [`Stored::new`] and [`Capabilities::new`], and holds the
/// save lock through [`lock`].
pub trait Store {
    /// What this store can do that not every store can.
    fn capabilities(&self) -> Capabilities;

    /// Deletes the file at `path` when `precondition` holds.
    ///
    /// # Errors
    ///
    /// `StoreError::Conflict` when `precondition` does not hold, which
    /// includes `Precondition::Matches` where no file is.
    /// `StoreError::NotFound` where no file is under `Precondition::None` or
    /// `Precondition::Absent`, except in the `MinIO` and AWS S3 stores, whose
    /// delete then succeeds. `StoreError::Backend` when the store fails.
    fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Creates the place this store keeps its files when it is missing, such
    /// as a bucket. By default, `StoreError::Unsupported`: the store has none.
    fn ensure_container(&self) -> impl Future<Output = Result<(), StoreError>> + Send {
        no_container()
    }

    /// Finishes each deletion of an embedded file that stopped halfway, and
    /// gives the path of each object it deleted. By default,
    /// `StoreError::Unsupported`: the store keeps no deletions.
    fn finish_deletions(&self) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        no_deletions()
    }

    /// Every file under `prefix`, or under the root with no prefix, at any
    /// depth, in byte order of their paths.
    ///
    /// The file store and the bucket stores leave lock files out. The `MongoDB`
    /// store lists documents only, not the files they embed.
    ///
    /// # Errors
    ///
    /// `StoreError::InvalidPath` when a name under `prefix` is not a store
    /// path, and `StoreError::Backend` when the store fails.
    fn list(
        &self,
        prefix: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send;

    /// The bytes of the file at `path`, and their revision.
    ///
    /// # Errors
    ///
    /// `StoreError::NotFound` where no file is, and `StoreError::Backend` when
    /// the store fails.
    fn read(&self, path: &StorePath) -> impl Future<Output = Result<Stored, StoreError>> + Send;

    /// The bytes of `file`, which the document at `_document` embeds. By
    /// default, the file at that path.
    ///
    /// # Errors
    ///
    /// `StoreError::NotFound` where the store holds no such file, and
    /// `StoreError::Backend` when the store fails.
    fn read_embedded(
        &self,
        _document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send
    where
        Self: Sync,
    {
        read_at_path(self, file)
    }

    /// Removes each expired lock under `under`, the whole store when `None`,
    /// and gives the path of each lock removed. By default,
    /// `StoreError::Unsupported`: the store keeps no locks.
    fn remove_expired_locks(
        &self,
        _under: Option<&StorePath>,
    ) -> impl Future<Output = Result<Vec<StorePath>, StoreError>> + Send {
        no_locks()
    }

    /// Writes `bytes` whole: a reader sees the old file or the new one, never a
    /// part of either.
    ///
    /// The write goes ahead only when `precondition` holds, and gives the new
    /// revision. It takes no lock, so it does not wait on a save.
    ///
    /// # Errors
    ///
    /// `StoreError::Conflict` when `precondition` does not hold,
    /// `StoreError::InvalidPath` for a path the store cannot hold, and
    /// `StoreError::Backend` when the store fails.
    fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send;

    /// Writes the document `text` at `path` and each file of `files`, which it
    /// embeds. By default, each file at its path, with no precondition, then the
    /// text under `precondition`; no file is deleted.
    ///
    /// `precondition` applies to the document only. The file store and the
    /// bucket stores hold the document's lock for the whole save (see
    /// [`lock`]); the `MongoDB` store keeps each file once, by its SHA-256.
    ///
    /// # Errors
    ///
    /// `StoreError::Conflict` when `precondition` does not hold or another
    /// save holds the document's lock, and the errors of [`Store::write`].
    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send
    where
        Self: Sync,
    {
        write_at_paths(self, path, text, files, precondition, || {
            future::ready(Ok(()))
        })
    }
}

/// A future a [`DynStore`] gives.
pub type BoxFuture<'future, T> = Pin<Box<dyn Future<Output = T> + Send + 'future>>;

/// [`Store`] as a trait object: the same operations, each returning a boxed
/// future, so a program can hold whichever store it opened in a
/// `Box<dyn DynStore>`. Every `Store` that is `Send + Sync` is one.
pub trait DynStore: Send + Sync {
    /// As [`Store::capabilities`].
    fn capabilities(&self) -> Capabilities;

    /// As [`Store::delete`].
    fn delete<'call>(
        &'call self,
        path: &'call StorePath,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<(), StoreError>>;

    /// As [`Store::ensure_container`].
    fn ensure_container(&self) -> BoxFuture<'_, Result<(), StoreError>>;

    /// As [`Store::finish_deletions`].
    fn finish_deletions(&self) -> BoxFuture<'_, Result<Vec<StorePath>, StoreError>>;

    /// As [`Store::list`].
    fn list<'call>(
        &'call self,
        prefix: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>>;

    /// As [`Store::read`].
    fn read<'call>(
        &'call self,
        path: &'call StorePath,
    ) -> BoxFuture<'call, Result<Stored, StoreError>>;

    /// As [`Store::read_embedded`].
    fn read_embedded<'call>(
        &'call self,
        document: &'call StorePath,
        file: &'call StorePath,
    ) -> BoxFuture<'call, Result<Vec<u8>, StoreError>>;

    /// As [`Store::remove_expired_locks`].
    fn remove_expired_locks<'call>(
        &'call self,
        under: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>>;

    /// As [`Store::write`].
    fn write<'call>(
        &'call self,
        path: &'call StorePath,
        bytes: &'call [u8],
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>>;

    /// As [`Store::write_with_embedded`].
    fn write_with_embedded<'call>(
        &'call self,
        path: &'call StorePath,
        text: &'call str,
        files: &'call BTreeMap<StorePath, Vec<u8>>,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>>;
}

impl<S: Store + Send + Sync> DynStore for S {
    fn capabilities(&self) -> Capabilities {
        Store::capabilities(self)
    }

    fn delete<'call>(
        &'call self,
        path: &'call StorePath,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<(), StoreError>> {
        Box::pin(Store::delete(self, path, precondition))
    }

    fn ensure_container(&self) -> BoxFuture<'_, Result<(), StoreError>> {
        Box::pin(Store::ensure_container(self))
    }

    fn finish_deletions(&self) -> BoxFuture<'_, Result<Vec<StorePath>, StoreError>> {
        Box::pin(Store::finish_deletions(self))
    }

    fn list<'call>(
        &'call self,
        prefix: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>> {
        Box::pin(Store::list(self, prefix))
    }

    fn read<'call>(
        &'call self,
        path: &'call StorePath,
    ) -> BoxFuture<'call, Result<Stored, StoreError>> {
        Box::pin(Store::read(self, path))
    }

    fn read_embedded<'call>(
        &'call self,
        document: &'call StorePath,
        file: &'call StorePath,
    ) -> BoxFuture<'call, Result<Vec<u8>, StoreError>> {
        Box::pin(Store::read_embedded(self, document, file))
    }

    fn remove_expired_locks<'call>(
        &'call self,
        under: Option<&'call StorePath>,
    ) -> BoxFuture<'call, Result<Vec<StorePath>, StoreError>> {
        Box::pin(Store::remove_expired_locks(self, under))
    }

    fn write<'call>(
        &'call self,
        path: &'call StorePath,
        bytes: &'call [u8],
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>> {
        Box::pin(Store::write(self, path, bytes, precondition))
    }

    fn write_with_embedded<'call>(
        &'call self,
        path: &'call StorePath,
        text: &'call str,
        files: &'call BTreeMap<StorePath, Vec<u8>>,
        precondition: &'call Precondition,
    ) -> BoxFuture<'call, Result<Revision, StoreError>> {
        Box::pin(Store::write_with_embedded(
            self,
            path,
            text,
            files,
            precondition,
        ))
    }
}

/// A `dyn DynStore` is a `Store` again, so the document functions take it.
///
/// Each method awaits the boxed future in an `async` body: returned as it is,
/// its one lifetime, shared by every reference of the call, is not one the
/// trait's `impl Future` can name.
impl Store for dyn DynStore + '_ {
    fn capabilities(&self) -> Capabilities {
        DynStore::capabilities(self)
    }

    async fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        DynStore::delete(self, path, precondition).await
    }

    async fn ensure_container(&self) -> Result<(), StoreError> {
        DynStore::ensure_container(self).await
    }

    async fn finish_deletions(&self) -> Result<Vec<StorePath>, StoreError> {
        DynStore::finish_deletions(self).await
    }

    async fn list(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        DynStore::list(self, prefix).await
    }

    async fn read(&self, path: &StorePath) -> Result<Stored, StoreError> {
        DynStore::read(self, path).await
    }

    async fn read_embedded(
        &self,
        document: &StorePath,
        file: &StorePath,
    ) -> Result<Vec<u8>, StoreError>
    where
        Self: Sync,
    {
        DynStore::read_embedded(self, document, file).await
    }

    async fn remove_expired_locks(
        &self,
        under: Option<&StorePath>,
    ) -> Result<Vec<StorePath>, StoreError> {
        DynStore::remove_expired_locks(self, under).await
    }

    async fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        DynStore::write(self, path, bytes, precondition).await
    }

    async fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError>
    where
        Self: Sync,
    {
        DynStore::write_with_embedded(self, path, text, files, precondition).await
    }
}

/// What `Store::ensure_container` does by default.
pub(crate) fn no_container() -> future::Ready<Result<(), StoreError>> {
    future::ready(Err(StoreError::Unsupported("create its container")))
}

/// What `Store::finish_deletions` does by default.
pub(crate) fn no_deletions() -> future::Ready<Result<Vec<StorePath>, StoreError>> {
    future::ready(Err(StoreError::Unsupported("finish deletions")))
}

/// What `Store::remove_expired_locks` does by default.
pub(crate) fn no_locks() -> future::Ready<Result<Vec<StorePath>, StoreError>> {
    future::ready(Err(StoreError::Unsupported("remove expired locks")))
}

/// What `Store::read_embedded` does by default: the bytes of the file at
/// `file`.
pub(crate) async fn read_at_path<S>(store: &S, file: &StorePath) -> Result<Vec<u8>, StoreError>
where
    S: Store + Sync + ?Sized,
{
    Ok(store.read(file).await?.bytes)
}

/// What `Store::write_with_embedded` does by default: each file of `files` at
/// its path, with no precondition, then `text` at `path` under `precondition`,
/// each write after `check` passes.
pub(crate) async fn write_at_paths<S, C, F>(
    store: &S,
    path: &StorePath,
    text: &str,
    files: &BTreeMap<StorePath, Vec<u8>>,
    precondition: &Precondition,
    check: C,
) -> Result<Revision, StoreError>
where
    S: Store + Sync + ?Sized,
    C: Fn() -> F + Send + Sync,
    F: Future<Output = Result<(), StoreError>> + Send,
{
    for (file, bytes) in files {
        check().await?;
        store.write(file, bytes, &Precondition::None).await?;
    }
    check().await?;
    store.write(path, text.as_bytes(), precondition).await
}

#[cfg(test)]
mod tests;
