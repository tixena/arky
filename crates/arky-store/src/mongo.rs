//! Files kept in a MongoDB collection, one record each, and the files they
//! embed kept as objects of a `MinIO` bucket, each named by a record of a
//! second collection.
//!
//! This module needs the `mongodb` feature, which turns on `minio`.
//! [`MongoStore`] keeps each document as its parsed tree, its frontmatter as
//! plain BSON that a query can name; [`MongoStore::find`] takes such a query,
//! and [`record::frontmatter_is`] builds one. Each file a document embeds is
//! kept once, by the SHA-256 of its bytes, in an [`ObjectStore`] over a
//! `MinioBucket`, and is deleted once no document embeds it. The [`record`]
//! module holds the record types and the functions that convert them, which
//! need no server.
//!
//! A `MongoStore` needs a `MongoDB` server and an S3-compatible server for the
//! files, and its futures need tokio with its timer on: a save that finds a
//! file's record held by a deletion tries up to 20 times, 50 milliseconds
//! apart. It takes no lock.

pub mod record;

use alloc::collections::BTreeMap;
use core::fmt;
use core::str::from_utf8;
use core::time::Duration;

use arky::bundle::{Missing, embedded_paths, media_type};
use arky::forms::{Form, decode};
use arky::parser::ast::RawDocument;
use arky::parser::parse;
use arky::writer::{self, WriteOptions};
use bson::oid::ObjectId;
use bson::{Document, RawDocumentBuf, doc};
use mongodb::error::{Error, ErrorKind, WriteFailure};
use mongodb::options::IndexOptions;
use mongodb::{Client, Collection, IndexModel};
use os_shim::bucket::MinioBucket;
use serde::Deserialize;
use tokio::time;

use crate::document::{is_note, parent_of};
use crate::file;
use crate::mongo::record::stored_object_schema::{Filter, Update, WriteError};
use crate::mongo::record::{
    Claim, NO_DOCUMENT, StoredDocument, StoredObject, from_record, revision_of, to_record,
};
use crate::object::ObjectStore;
use crate::{Capabilities, Precondition, Revision, Store, StoreError, StorePath, Stored};

/// How many times a save tries to claim an object whose record a deletion
/// holds.
const CLAIM_TRIES: u32 = 20;

/// How long a save waits between two of those tries.
const CLAIM_WAIT: Duration = Duration::from_millis(50);

/// The code of a write that breaks a unique index.
const DUPLICATE_KEY: i32 = 11000;

/// The code of a command on a collection that does not exist.
const NAMESPACE_NOT_FOUND: i32 = 26;

/// A file a save writes: its path beside the document, its path in the
/// bundle, its bytes, their SHA-256, the path of its object, and the alt text
/// of the first embed that names it.
struct Embedded<'save> {
    bytes: &'save [u8],
    description: String,
    hash: String,
    name: &'save str,
    object: StorePath,
    path: &'save StorePath,
}

/// The `files` of a document's record, read alone.
#[derive(Deserialize)]
struct HeldFiles {
    #[serde(default)]
    files: BTreeMap<String, String>,
}

/// The `revision` of a document's record, read alone.
#[derive(Deserialize)]
struct HeldRevision {
    revision: String,
}

/// The `save` of a document's record, read alone.
#[derive(Deserialize)]
struct HeldSave {
    #[serde(default)]
    save: Option<ObjectId>,
}

/// The save whose claims a release takes back: one save, the save of a record
/// written before records named one, or none where a save inserted its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Replaced {
    Nothing,
    Save(ObjectId),
    Unnamed,
}

/// The files of one collection of one database, each one record, with a
/// unique index on `path`.
///
/// Each file they embed is the object `files/<sha256>` of `files`, named by
/// one record of `<collection>_files`, which has an index on
/// `embedded_by.path`. An object nothing embeds is deleted, then its record.
///
/// A record holds the parsed document, not its bytes: [`Store::read`] gives
/// it back in arky's form, a valid frontmatter in YAML, whatever syntax it was
/// written in, and a frontmatter no entry of reads as it was written, in its
/// own syntax. Its revision is what [`record::revision_of`] gives for that
/// text and the SHA-256 of each file it embeds, so a changed picture changes
/// the document's revision. A read of the path of an embedded file gives that
/// file's bytes, but [`Store::list`] gives the paths of documents only. A
/// record nests at most 100 levels, so a document of 13 nested list items is
/// refused.
pub struct MongoStore<'system> {
    collection: Collection<Document>,
    files: ObjectStore<'system, MinioBucket>,
    objects: Collection<Document>,
}

impl<'system> MongoStore<'system> {
    /// Claims the object of `file` for the document at `path`, by `save`,
    /// trying again while a deletion holds its record.
    async fn claim(
        &self,
        path: &StorePath,
        file: &Embedded<'_>,
        save: ObjectId,
    ) -> Result<(), StoreError> {
        let mut tries = 1;
        while !self.try_claim(path, file, save).await? {
            if tries == CLAIM_TRIES {
                return Err(StoreError::Backend(format!(
                    "{}: a deletion of its object has not finished; `arky asset finish-deletions` finishes it once no other arky writes to this store",
                    file.path
                )));
            }
            tries += 1;
            time::sleep(CLAIM_WAIT).await;
        }
        Ok(())
    }

    /// Claims the object whose bytes have the SHA-256 `hash`, which the record
    /// at `path` names at `name`, by `save`: `false` when its record is gone or
    /// a deletion holds it.
    async fn claim_kept(
        &self,
        path: &StorePath,
        name: &str,
        hash: &str,
        save: ObjectId,
    ) -> Result<bool, StoreError> {
        let file = path.beside(name)?;
        let (filter, update) =
            adding(hash, path, &file, save).map_err(|error| failed(&file, &error))?;
        let claimed = self
            .objects
            .update_one(filter, update.into_document())
            .await
            .map_err(|error| failed(&file, &error))?;
        Ok(claimed.matched_count == 1)
    }

    /// Deletes the object whose bytes have the SHA-256 `hash`, then its
    /// record, when nothing embeds it and no other deletion holds it.
    async fn delete_object(&self, hash: &str) -> Result<(), StoreError> {
        let object = object_path(hash)?;
        self.delete_unembedded(hash)
            .await
            .map_err(|error| stopped_deletion(&object, &error))
    }

    /// `delete_object` with the error of the step that failed.
    async fn delete_unembedded(&self, hash: &str) -> Result<(), StoreError> {
        if self.mark(hash).await? {
            self.remove_object(hash).await?;
            self.remove_record(hash).await?;
        }
        Ok(())
    }

    /// The object that the record of `document` names for `file` in its
    /// `files`: `NotFound` when no record is at `document`, its `files` do not
    /// hold `file`, or the bucket does not hold the object.
    async fn embedded_file(
        &self,
        document: &StorePath,
        file: &StorePath,
    ) -> Result<Vec<u8>, StoreError> {
        let missing = || StoreError::NotFound(file.clone());
        let name = name_in(document, file.as_str()).ok_or_else(missing)?;
        let held = self.held_files(document).await?;
        let hash = held.get(name).ok_or_else(missing)?;
        match self.files.read(&object_path(hash)?).await {
            Ok(object) => Ok(object.bytes),
            Err(StoreError::NotFound(_)) => Err(missing()),
            Err(error) => Err(error),
        }
    }

    /// Creates the bucket of the store's files when it does not exist, and
    /// leaves a bucket that exists as it is.
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` as [`ObjectStore::ensure`] gives it.
    pub async fn ensure_files_bucket(&self) -> Result<(), StoreError> {
        self.files.ensure().await
    }

    /// Every record `filter` matches, read through `from_record`.
    ///
    /// A record's fields are `path`, `revision`, `save`, `files` and `doc`,
    /// the document's tree, whose frontmatter is `doc.data`: the filter
    /// `doc! { "doc.data.status": "open" }` matches the documents whose
    /// `status` is `open`. [`record::frontmatter_is`] builds that filter for
    /// any key and value.
    ///
    /// Write a found record's `document` with [`writer::write`] for its text,
    /// or call [`document::get`](crate::document::get) with
    /// `StorePath::new(&stored.path)?` for the document with its files.
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` with the driver's message, or for a record that
    /// does not read.
    pub async fn find(&self, filter: Document) -> Result<Vec<StoredDocument>, StoreError> {
        let mut records = self
            .collection
            .find(filter)
            .await
            .map_err(|error| backend(&error))?;
        let mut found = Vec::new();
        while records.advance().await.map_err(|error| backend(&error))? {
            found.push(from_record(record_of(records.current())?)?);
        }
        Ok(found)
    }

    /// Finishes each deletion that stopped after it marked its record: deletes
    /// the object, then the record, and gives the SHA-256 of each, sorted.
    ///
    /// Until then, a save of a document that embeds the same bytes fails,
    /// naming the deletion. [`object_path`] gives the path of each object
    /// deleted. Run it only while nothing else writes to the store.
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` when MongoDB or the bucket fails.
    pub async fn finish_deletions(&self) -> Result<Vec<String>, StoreError> {
        let held = StoredObject::MONGO_FIELDS
            .deleting()
            .eq(true)
            .map_err(|error| backend(&error))?;
        let records = StoredObject::find(&self.objects, held)
            .await
            .map_err(|error| backend(&error))?;
        let mut finished = Vec::new();
        for record in records {
            self.remove_object(&record.id).await?;
            self.remove_record(&record.id).await?;
            finished.push(record.id);
        }
        finished.sort_unstable();
        Ok(finished)
    }

    /// Takes back each claim on the document at `path` that `save` made, and
    /// deletes each object left unembedded: the save's document was not stored.
    async fn give_back(&self, path: &StorePath, save: ObjectId) -> Result<(), StoreError> {
        for hash in self.release(path, &Replaced::Save(save)).await? {
            self.delete_unembedded(&hash)
                .await
                .map_err(|error| failed(path, &error))?;
        }
        Ok(())
    }

    /// The `files` of the record at `path`, read alone: none when no record
    /// is there.
    async fn held_files(&self, path: &StorePath) -> Result<BTreeMap<String, String>, StoreError> {
        let Some(record) = self
            .collection
            .find_one(on_path(path)?)
            .projection(doc! { "_id": 0_i32, "files": 1_i32 })
            .await
            .map_err(|error| failed(path, &error))?
        else {
            return Ok(BTreeMap::new());
        };
        let held: HeldFiles =
            bson::deserialize_from_document(record).map_err(|error| failed(path, &error))?;
        Ok(held.files)
    }

    /// Marks the record of the object whose bytes have the SHA-256 `hash` as
    /// held by a deletion, only while nothing embeds it and no other deletion
    /// holds it: `false` when no record was marked.
    async fn mark(&self, hash: &str) -> Result<bool, StoreError> {
        let object = object_path(hash)?;
        let (filter, update) = marking(hash).map_err(|error| failed(&object, &error))?;
        let marked = self
            .objects
            .update_one(filter, update)
            .await
            .map_err(|error| failed(&object, &error))?;
        Ok(marked.matched_count == 1)
    }

    /// Connects with `uri`, the connection string, keeps each embedded file
    /// in `files`, and makes the unique index on `path` and the index on
    /// `embedded_by.path` of `<collection>_files` where they are missing.
    ///
    /// It reads the indexes at once, so the server must be reachable, and it
    /// runs inside a tokio runtime. `files` is usually
    /// [`ObjectStore::open`]'s store over the bucket that keeps the files:
    ///
    /// ```rust
    /// use arky_store::StoreError;
    /// use arky_store::mongo::MongoStore;
    /// use arky_store::object::{ObjectConfig, ObjectStore};
    /// use os_shim::System;
    ///
    /// async fn open_store(system: &dyn System) -> Result<MongoStore<'_>, StoreError> {
    ///     let config = ObjectConfig::new(
    ///         "http://127.0.0.1:9000".to_owned(),
    ///         "us-east-1".to_owned(),
    ///         "notes".to_owned(),
    ///         None,
    ///     );
    ///     let files = ObjectStore::open(system, config)?;
    ///     MongoStore::open("mongodb://localhost:27017", "notes", "documents", files).await
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// `StoreError::Backend` with the driver's message.
    pub async fn open(
        uri: &str,
        database: &str,
        collection: &str,
        files: ObjectStore<'system, MinioBucket>,
    ) -> Result<Self, StoreError> {
        let client = Client::with_uri_str(uri)
            .await
            .map_err(|error| backend(&error))?;
        let store = Self {
            collection: client.database(database).collection(collection),
            files,
            objects: client
                .database(database)
                .collection(&format!("{collection}_files")),
        };
        let indexes = [
            (&store.collection, "path", true),
            (&store.objects, "embedded_by.path", false),
        ];
        for (indexed, key, unique) in indexes {
            if !has_index(indexed, key, unique).await? {
                let mut keys = Document::new();
                keys.insert(key, 1_i32);
                let index = IndexModel::builder()
                    .keys(keys)
                    .options(IndexOptions::builder().unique(unique).build())
                    .build();
                indexed
                    .create_index(index)
                    .await
                    .map_err(|error| backend(&error))?;
            }
        }
        Ok(store)
    }

    /// The document at `path`, with its revision.
    ///
    /// The tree is read back from BSON: each node's `kind` is empty, and each
    /// frontmatter value is what BSON held. A date-time comes back as an
    /// offset date-time in UTC, to the millisecond, and one at midnight UTC as
    /// a date.
    ///
    /// # Errors
    ///
    /// `StoreError::NotFound` when no record is at `path`, or
    /// `StoreError::Backend` with the driver's message or for a record that
    /// does not read.
    pub async fn read_document(
        &self,
        path: &StorePath,
    ) -> Result<(RawDocument, Revision), StoreError> {
        let raw = self
            .collection
            .clone_with_type::<RawDocumentBuf>()
            .find_one(on_path(path)?)
            .await
            .map_err(|error| failed(path, &error))?
            .ok_or_else(|| StoreError::NotFound(path.clone()))?;
        let stored = from_record(record_of(&raw)?)?;
        Ok((stored.document, Revision(stored.revision)))
    }

    /// The bytes of the file at `path`, where no document is: the object that
    /// the record of a document claiming the path names.
    async fn read_file(&self, path: &StorePath) -> Result<Vec<u8>, StoreError> {
        let claimed = Claim::MONGO_FIELDS
            .path()
            .eq(path.as_str().to_owned())
            .map_err(|error| failed(path, &error))?;
        let records = StoredObject::find(
            &self.objects,
            StoredObject::MONGO_FIELDS.embedded_by().nest(claimed),
        )
        .await
        .map_err(|error| failed(path, &error))?;
        for record in records {
            for claim in record
                .embedded_by
                .iter()
                .filter(|claim| claim.path == path.as_str())
            {
                match self
                    .read_embedded(&StorePath::new(&claim.document)?, path)
                    .await
                {
                    Err(StoreError::NotFound(_)) => {}
                    read => return read,
                }
            }
        }
        Err(StoreError::NotFound(path.clone()))
    }

    /// The revision of the record at `path`, or `NO_DOCUMENT` where none is.
    async fn read_revision(&self, path: &StorePath) -> Result<Revision, StoreError> {
        let Some(record) = self
            .collection
            .find_one(on_path(path)?)
            .projection(doc! { "_id": 0_i32, "revision": 1_i32 })
            .await
            .map_err(|error| failed(path, &error))?
        else {
            return Ok(Revision(NO_DOCUMENT.to_owned()));
        };
        let held: HeldRevision =
            bson::deserialize_from_document(record).map_err(|error| failed(path, &error))?;
        Ok(Revision(held.revision))
    }

    /// Takes back each claim on the document at `path` that `of` made, and gives
    /// the SHA-256 of each object it took a claim from.
    async fn release(&self, path: &StorePath, of: &Replaced) -> Result<Vec<String>, StoreError> {
        let save = match *of {
            Replaced::Nothing => return Ok(Vec::new()),
            Replaced::Save(save) => Some(save),
            Replaced::Unnamed => None,
        };
        let paths = StoredObject::MONGO_FIELDS;
        let held = claims_of(path, save).map_err(|error| failed(path, &error))?;
        let mut records = self
            .objects
            .find(paths.embedded_by().elem_match(held).into_document())
            .projection(doc! { "_id": 1_i32 })
            .await
            .map_err(|error| failed(path, &error))?;
        let mut released = Vec::new();
        while records
            .advance()
            .await
            .map_err(|error| failed(path, &error))?
        {
            let hash = records
                .current()
                .get_str("_id")
                .map_err(|error| failed(path, &error))?;
            released.push(hash.to_owned());
        }
        let pulled = claims_of(path, save).map_err(|error| failed(path, &error))?;
        let pull = paths.embedded_by().pull(pulled).into_document();
        for hash in &released {
            let on_id = paths
                .id()
                .eq(hash.clone())
                .map_err(|error| failed(path, &error))?;
            self.objects
                .update_one(on_id.into_document(), pull.clone())
                .await
                .map_err(|error| failed(path, &error))?;
        }
        Ok(released)
    }

    /// Deletes the record of the document at `path` if `precondition` holds,
    /// as `Store::delete` says.
    async fn remove_document(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        let (filter, unmatched) = match precondition {
            Precondition::Absent => {
                let held = self
                    .collection
                    .find_one(on_path(path)?)
                    .projection(doc! { "_id": 1_i32 })
                    .await
                    .map_err(|error| failed(path, &error))?;
                return Err(held.map_or_else(
                    || StoreError::NotFound(path.clone()),
                    |_| StoreError::Conflict(path.clone()),
                ));
            }
            Precondition::Matches(expected) => (
                on_revision(path, expected)?,
                StoreError::Conflict(path.clone()),
            ),
            Precondition::None => (on_path(path)?, StoreError::NotFound(path.clone())),
        };
        let deleted = self
            .collection
            .delete_one(filter)
            .await
            .map_err(|error| failed(path, &error))?;
        if deleted.deleted_count == 0 {
            Err(unmatched)
        } else {
            Ok(())
        }
    }

    /// Deletes the record at `path` where it is still at `read`, and gives the
    /// save it names. `StoreError::Conflict` when the record is no longer at
    /// `read`.
    async fn remove_document_at(
        &self,
        path: &StorePath,
        read: &Revision,
    ) -> Result<Replaced, StoreError> {
        let removed = self
            .collection
            .find_one_and_delete(on_revision(path, read)?)
            .projection(doc! { "_id": 0_i32, "save": 1_i32 })
            .await
            .map_err(|error| failed(path, &error))?
            .ok_or_else(|| StoreError::Conflict(path.clone()))?;
        named_by(path, removed)
    }

    /// Deletes the object whose bytes have the SHA-256 `hash` from the bucket.
    async fn remove_object(&self, hash: &str) -> Result<(), StoreError> {
        self.files
            .delete(&object_path(hash)?, &Precondition::None)
            .await
    }

    /// Deletes the record of the object whose bytes have the SHA-256 `hash`,
    /// only while a deletion holds it.
    async fn remove_record(&self, hash: &str) -> Result<(), StoreError> {
        let object = object_path(hash)?;
        let paths = StoredObject::MONGO_FIELDS;
        let held = paths
            .id()
            .eq(hash.to_owned())
            .and_then(|on_id| Ok(on_id.and(paths.deleting().eq(true)?)))
            .map_err(|error| failed(&object, &error))?;
        self.objects
            .delete_one(held.into_document())
            .await
            .map_err(|error| failed(&object, &error))?;
        Ok(())
    }

    /// The revision a save or a deletion under `precondition` takes as read:
    /// the caller's for `Matches`, `NO_DOCUMENT` for `Absent`, and the record's
    /// for `None`.
    async fn revision_read(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        match precondition {
            Precondition::Absent => Ok(Revision(NO_DOCUMENT.to_owned())),
            Precondition::Matches(expected) => Ok(expected.clone()),
            Precondition::None => self.read_revision(path).await,
        }
    }

    /// Writes the record of `document` where the record at `path` is still at
    /// `read`, naming `save`, with `files`, the SHA-256 of each file it embeds
    /// by its path in the bundle, and gives the revision written and the save
    /// the replaced record names: `Replaced::Nothing` when it inserted.
    /// `StoreError::Conflict` when the record is no longer at `read`.
    async fn save(
        &self,
        path: &StorePath,
        document: &RawDocument,
        files: &BTreeMap<String, String>,
        read: &Revision,
        save: ObjectId,
    ) -> Result<(Revision, Replaced), StoreError> {
        let text = writer::write(document, &WriteOptions::default())
            .map_err(|error| failed(path, &error))?;
        let revision = revision_of(&text, files);
        let mut record = to_record(&StoredDocument {
            document: document.clone(),
            files: files.clone(),
            id: ObjectId::new(),
            path: path.as_str().to_owned(),
            revision: revision.0.clone(),
            save: Some(save),
        })?;
        if read.0 == NO_DOCUMENT {
            self.collection.insert_one(record).await.map_err(|error| {
                if duplicate_key(&error) {
                    StoreError::Conflict(path.clone())
                } else {
                    failed(path, &error)
                }
            })?;
            return Ok((revision, Replaced::Nothing));
        }
        // A replacement keeps the `_id` of the record it replaces.
        record.remove("_id");
        let replaced = self
            .collection
            .find_one_and_replace(on_revision(path, read)?, record)
            .projection(doc! { "_id": 0_i32, "save": 1_i32 })
            .await
            .map_err(|error| failed(path, &error))?
            .ok_or_else(|| StoreError::Conflict(path.clone()))?;
        Ok((revision, named_by(path, replaced)?))
    }

    /// Each entry of the `files` of the record at `path` whose file `document`
    /// embeds, an embedded note's own embeds included, each note read from its
    /// object.
    async fn still_embedded(
        &self,
        path: &StorePath,
        document: &RawDocument,
    ) -> Result<BTreeMap<String, String>, StoreError> {
        let held = self.held_files(path).await?;
        let mut kept = BTreeMap::new();
        let mut pending = vec![(String::new(), document.clone())];
        while let Some((folder, text)) = pending.pop() {
            let named = embedded_paths(&text, &folder).map_err(|error| failed(path, &error))?;
            for (_, name) in named {
                if kept.contains_key(&name) {
                    continue;
                }
                let file = match path.beside(&name) {
                    Ok(file) => file,
                    // `document::get` reads no file at a path that is not a store path.
                    Err(StoreError::InvalidPath(_)) => continue,
                    Err(error) => return Err(error),
                };
                let Some(hash) = held.get(&name) else {
                    continue;
                };
                if is_note(&name) {
                    match self.files.read(&object_path(hash)?).await {
                        Ok(object) => {
                            let note =
                                from_utf8(&object.bytes).map_err(|error| failed(&file, &error))?;
                            pending.push((parent_of(&name).to_owned(), parse(note).document));
                        }
                        Err(StoreError::NotFound(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                kept.insert(name, hash.clone());
            }
        }
        Ok(kept)
    }

    /// Saves `document` at `path` with `files` where the record is still at
    /// `read`, naming `save`, then tidies the claims of the save the replaced
    /// record names; a save that loses gives back its own claims and is a
    /// conflict.
    async fn store(
        &self,
        path: &StorePath,
        document: &RawDocument,
        files: &BTreeMap<String, String>,
        read: &Revision,
        save: ObjectId,
    ) -> Result<Revision, StoreError> {
        match self.save(path, document, files, read, save).await {
            Ok((wrote, replaced)) => {
                self.tidy(path, &replaced).await?;
                Ok(wrote)
            }
            Err(StoreError::Conflict(_)) => {
                self.give_back(path, save).await?;
                Err(StoreError::Conflict(path.clone()))
            }
            Err(error) => Err(error),
        }
    }

    /// Takes back each claim on the document at `path` that `of` made, and
    /// deletes each object left unembedded, once the document's change is
    /// stored.
    async fn tidy(&self, path: &StorePath, of: &Replaced) -> Result<(), StoreError> {
        let released = self
            .release(path, of)
            .await
            .map_err(|error| stored_release_failed(path, &error))?;
        for hash in released {
            self.delete_object(&hash).await?;
        }
        Ok(())
    }

    /// One try at claiming the object of `file` for the document at `path`, by
    /// `save`: its record gains the claim, and is made with the object's
    /// metadata where none is. `false` while a deletion holds the record.
    async fn try_claim(
        &self,
        path: &StorePath,
        file: &Embedded<'_>,
        save: ObjectId,
    ) -> Result<bool, StoreError> {
        let (filter, update) =
            claiming(path, file, save).map_err(|error| failed(file.path, &error))?;
        match self.objects.update_one(filter, update).upsert(true).await {
            Ok(_) => Ok(true),
            Err(error) if duplicate_key(&error) => Ok(false),
            Err(error) => Err(failed(file.path, &error)),
        }
    }

    /// Puts the object of `file` in the bucket unless its record says it is
    /// stored, then marks the record stored.
    async fn upload(&self, file: &Embedded<'_>) -> Result<(), StoreError> {
        let paths = StoredObject::MONGO_FIELDS;
        let stored = paths
            .id()
            .eq(file.hash.clone())
            .and_then(|on_id| Ok(on_id.and(paths.stored().eq(true)?)))
            .map_err(|error| failed(file.path, &error))?;
        let held = self
            .objects
            .find_one(stored.into_document())
            .projection(doc! { "_id": 1_i32 })
            .await
            .map_err(|error| failed(file.path, &error))?;
        if held.is_some() {
            return Ok(());
        }
        match self
            .files
            .write(&file.object, file.bytes, &Precondition::Absent)
            .await
        {
            // A conflict is the same bytes, put there by an earlier save.
            Ok(_) | Err(StoreError::Conflict(_)) => {}
            Err(error) => return Err(error),
        }
        let (on_id, marked) = paths
            .id()
            .eq(file.hash.clone())
            .and_then(|on_id| Ok((on_id, paths.stored().set(true)?)))
            .map_err(|error| failed(file.path, &error))?;
        self.objects
            .update_one(on_id.into_document(), marked.into_document())
            .await
            .map_err(|error| failed(file.path, &error))?;
        Ok(())
    }

    /// Writes `document` at `path` if `precondition` holds, and gives the new
    /// revision: the SHA-256 of the document in arky's form and of the files
    /// it embeds.
    ///
    /// The record keeps each file of the record it replaces that `document`
    /// still embeds, claimed again by this write, the claims of the save the
    /// replaced record names are taken back, and each object that leaves
    /// unembedded is deleted. Nothing is uploaded.
    ///
    /// # Errors
    ///
    /// `StoreError::Conflict` when `precondition` does not hold, when the record
    /// changed since the write read it, or when a kept file's record is gone or
    /// held by a deletion; or `StoreError::Backend` naming the path when the
    /// record nests too deep or the driver or the bucket fails, naming a kept
    /// note that is not UTF-8, or naming an object whose deletion fails.
    pub async fn write_document(
        &self,
        path: &StorePath,
        document: &RawDocument,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        let read = self.revision_read(path, precondition).await?;
        let save = ObjectId::new();
        let kept = self.still_embedded(path, document).await?;
        for (name, hash) in &kept {
            if !self.claim_kept(path, name, hash, save).await? {
                self.give_back(path, save).await?;
                return Err(StoreError::Conflict(path.clone()));
            }
        }
        self.store(path, document, &kept, &read, save).await
    }

    /// Reads the revision the write starts from, draws the id of the save,
    /// claims the object of each file of `files` for the document by it,
    /// uploads each one its record does not say is stored, saves the document
    /// naming the save, with the SHA-256 of each file, where the record is still
    /// at that revision, takes back the claims of the save the replaced record
    /// names, then deletes each object that leaves unembedded. A write that
    /// finds the record changed gives back its own claims and is a conflict.
    async fn write_with_files(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        let document = parse(text).document;
        let embedded = embedded(path, &document, files)?;
        let read = self.revision_read(path, precondition).await?;
        let save = ObjectId::new();
        for file in &embedded {
            self.claim(path, file, save).await?;
        }
        for file in &embedded {
            self.upload(file).await?;
        }
        let hashes = embedded
            .iter()
            .map(|file| (file.name.to_owned(), file.hash.clone()))
            .collect();
        self.store(path, &document, &hashes, &read, save).await
    }
}

impl Store for MongoStore<'_> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            atomic_preconditions: true,
        }
    }

    /// With `Precondition::Absent`, nothing is deleted: a record at `path` is
    /// `Conflict`, and none is `NotFound`. Otherwise the record is deleted only
    /// at the revision read, then each claim of the save it names is taken
    /// back, and each object that leaves unembedded is deleted.
    async fn delete(
        &self,
        path: &StorePath,
        precondition: &Precondition,
    ) -> Result<(), StoreError> {
        if matches!(precondition, Precondition::Absent) {
            return self.remove_document(path, precondition).await;
        }
        let read = self.revision_read(path, precondition).await?;
        if read.0 == NO_DOCUMENT {
            return Err(StoreError::NotFound(path.clone()));
        }
        let removed = self.remove_document_at(path, &read).await?;
        self.tidy(path, &removed).await
    }

    /// The path of every document under `prefix`, sorted. The files the
    /// documents embed are not listed.
    async fn list(&self, prefix: Option<&StorePath>) -> Result<Vec<StorePath>, StoreError> {
        let filter = prefix.map_or_else(Document::new, |folder| {
            let pattern = format!("^{}/", escaped(folder.as_str()));
            StoredDocument::MONGO_FIELDS
                .path()
                .regex(&pattern, "")
                .into_document()
        });
        let mut records = self
            .collection
            .find(filter)
            .projection(doc! { "_id": 0_i32, "path": 1_i32 })
            .await
            .map_err(|error| backend(&error))?;
        let mut paths = Vec::new();
        while records.advance().await.map_err(|error| backend(&error))? {
            let record = records
                .deserialize_current()
                .map_err(|error| backend(&error))?;
            let path = record
                .get_str("path")
                .map_err(|error| StoreError::Backend(format!("a record's `path`: {error}")))?;
            paths.push(StorePath::new(path)?);
        }
        paths.sort_unstable();
        Ok(paths)
    }

    /// The document at `path` in arky's form: a valid frontmatter in YAML, and
    /// one no entry of reads as it was written.
    ///
    /// A path no document is at reads as an embedded file: the object that
    /// the record of a document claiming the path names, with the revision
    /// `sha256:` and the hex SHA-256 of its bytes.
    async fn read(&self, path: &StorePath) -> Result<Stored, StoreError> {
        let (document, revision) = match self.read_document(path).await {
            Ok(read) => read,
            Err(StoreError::NotFound(_)) => {
                let bytes = self.read_file(path).await?;
                return Ok(Stored {
                    revision: file::revision(&bytes),
                    bytes,
                });
            }
            Err(error) => return Err(error),
        };
        let text = writer::write(&document, &WriteOptions::default())
            .map_err(|error| failed(path, &error))?;
        Ok(Stored {
            bytes: text.into_bytes(),
            revision,
        })
    }

    /// The bytes of the object that the record of `document` names for `file`:
    /// `StoreError::NotFound` when no record is at `document`, the record
    /// names no such file, or the bucket does not hold the object.
    fn read_embedded(
        &self,
        document: &StorePath,
        file: &StorePath,
    ) -> impl Future<Output = Result<Vec<u8>, StoreError>> + Send {
        self.embedded_file(document, file)
    }

    /// Reads `bytes` as markdown and stores the tree through
    /// [`MongoStore::write_document`]. It uploads no file, so a picture
    /// written as a data URI is not kept; store files through
    /// `write_with_embedded` or [`document::put`](crate::document::put).
    async fn write(
        &self,
        path: &StorePath,
        bytes: &[u8],
        precondition: &Precondition,
    ) -> Result<Revision, StoreError> {
        let bundle =
            decode(bytes, Form::Markdown, Missing::Keep).map_err(|error| failed(path, &error))?;
        self.write_document(path, bundle.document(), precondition)
            .await
    }

    /// Stores the document and each file of `files` once, by its SHA-256,
    /// uploading only the objects the bucket does not hold yet, then deletes
    /// each object no document embeds any more. Every file must sit under the
    /// folder of `path`, or the save fails with `StoreError::Backend`.
    fn write_with_embedded(
        &self,
        path: &StorePath,
        text: &str,
        files: &BTreeMap<StorePath, Vec<u8>>,
        precondition: &Precondition,
    ) -> impl Future<Output = Result<Revision, StoreError>> + Send {
        self.write_with_files(path, text, files, precondition)
    }
}

/// The filter and the update that add the claim of the document at
/// `document` on `file`, by `save`, to the record of the object whose bytes
/// have the SHA-256 `hash`, once, while no deletion holds it.
fn adding(
    hash: &str,
    document: &StorePath,
    file: &StorePath,
    save: ObjectId,
) -> Result<(Document, Update<StoredObject>), WriteError> {
    let paths = StoredObject::MONGO_FIELDS;
    let claim = Claim {
        document: document.as_str().to_owned(),
        path: file.as_str().to_owned(),
        save: Some(save),
    };
    let filter = paths
        .id()
        .eq(hash.to_owned())?
        .and(paths.deleting().ne(true)?);
    let added = bson::serialize_to_bson(&claim)?;
    Ok((
        filter.into_document(),
        Update::<StoredObject>::raw(doc! { "$addToSet": { "embedded_by": added } }),
    ))
}

fn backend(error: &dyn fmt::Display) -> StoreError {
    StoreError::Backend(error.to_string())
}

/// The filter and the update of a claim of the object of `file` for the
/// document at `path`, by `save`: the claim added once, and the object's
/// metadata set where the update makes the record.
fn claiming(
    path: &StorePath,
    file: &Embedded<'_>,
    save: ObjectId,
) -> Result<(Document, Document), WriteError> {
    let paths = StoredObject::MONGO_FIELDS;
    let (filter, added) = adding(&file.hash, path, file.path, save)?;
    let update = added
        .and(
            paths
                .object()
                .set_on_insert(file.object.as_str().to_owned())?,
        )
        .and(
            paths
                .content_type()
                .set_on_insert(media_type(file.name).to_owned())?,
        )
        .and(paths.size().set_on_insert(file.bytes.len() as u64)?)
        .and(
            paths
                .description()
                .set_on_insert(file.description.clone())?,
        )
        .and(paths.stored().set_on_insert(false)?);
    Ok((filter, update.into_document()))
}

/// The filter over the claims on the document at `path` that `save` made, or
/// with none, those that name no save.
fn claims_of(path: &StorePath, save: Option<ObjectId>) -> Result<Filter<Claim>, WriteError> {
    let claims = Claim::MONGO_FIELDS;
    let on_document = claims.document().eq(path.as_str().to_owned())?;
    Ok(match save {
        Some(made) => on_document.and(claims.save().eq(made)?),
        // `null` matches a claim with no `save`, and the typed field writes no `null`.
        None => on_document.and(Filter::<Claim>::raw(doc! { "save": null })),
    })
}

fn command_code(error: &Error) -> Option<i32> {
    if let ErrorKind::Command(command) = error.kind.as_ref() {
        Some(command.code)
    } else {
        None
    }
}

/// The alt text of the first embed that names each file of `files`, by its
/// path in the bundle: the embeds of `document` first, then each embedded
/// note's own, from the note's folder.
fn descriptions(
    document: &RawDocument,
    files: &BTreeMap<&str, (&StorePath, &[u8])>,
) -> Result<BTreeMap<String, String>, StoreError> {
    let mut described = BTreeMap::new();
    let mut pending = vec![(String::new(), document.clone())];
    while let Some((folder, text)) = pending.pop() {
        for (embed, name) in embedded_paths(&text, &folder).map_err(|error| backend(&error))? {
            if described.contains_key(&name) {
                continue;
            }
            if is_note(&name)
                && let Some((file, bytes)) = files.get(name.as_str())
            {
                let note = from_utf8(bytes).map_err(|error| failed(file, &error))?;
                pending.push((parent_of(&name).to_owned(), parse(note).document));
            }
            described.insert(name, embed.alt);
        }
    }
    Ok(described)
}

fn duplicate_key(error: &Error) -> bool {
    matches!(
        error.kind.as_ref(),
        ErrorKind::Write(WriteFailure::WriteError(failure)) if failure.code == DUPLICATE_KEY
    )
}

/// Each file of `files` that a save of `document` at `path` writes.
fn embedded<'save>(
    path: &StorePath,
    document: &RawDocument,
    files: &'save BTreeMap<StorePath, Vec<u8>>,
) -> Result<Vec<Embedded<'save>>, StoreError> {
    let mut named = BTreeMap::new();
    for (file, bytes) in files {
        let name = name_in(path, file.as_str())
            .ok_or_else(|| StoreError::Backend(format!("{file}: not in the folder of {path}")))?;
        named.insert(name, (file, bytes.as_slice()));
    }
    let mut described = descriptions(document, &named)?;
    named
        .into_iter()
        .map(|(name, (file, bytes))| {
            let hash = file::sha256(bytes);
            Ok(Embedded {
                bytes,
                description: described.remove(name).unwrap_or_default(),
                object: object_path(&hash)?,
                hash,
                name,
                path: file,
            })
        })
        .collect()
}

/// `text` with a `\` before each character a regular expression reads as
/// more than itself.
fn escaped(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len());
    for character in text.chars() {
        if "\\^$.|?*+()[]{}".contains(character) {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern
}

fn failed(path: &StorePath, error: &dyn fmt::Display) -> StoreError {
    StoreError::Backend(format!("{path}: {error}"))
}

/// Whether an index of `collection` has `key` as its one key, and is unique
/// where `unique` asks it. A collection that does not exist has none.
async fn has_index(
    collection: &Collection<Document>,
    key: &str,
    unique: bool,
) -> Result<bool, StoreError> {
    let mut indexes = match collection.list_indexes().await {
        Ok(indexes) => indexes,
        Err(error) if command_code(&error) == Some(NAMESPACE_NOT_FOUND) => return Ok(false),
        Err(error) => return Err(backend(&error)),
    };
    while indexes.advance().await.map_err(|error| backend(&error))? {
        let index = indexes
            .deserialize_current()
            .map_err(|error| backend(&error))?;
        let unique_index = index.options.and_then(|options| options.unique) == Some(true);
        if (unique_index || !unique) && index.keys.len() == 1 && index.keys.contains_key(key) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The filter and the update that mark the record of the object whose bytes
/// have the SHA-256 `hash` as held by a deletion, where nothing embeds it and
/// no deletion holds it yet.
fn marking(hash: &str) -> Result<(Document, Document), WriteError> {
    let paths = StoredObject::MONGO_FIELDS;
    // Without `deleting`, a second deletion could remove what a later save uploads.
    let filter = paths
        .id()
        .eq(hash.to_owned())?
        .and(paths.embedded_by().size(0))
        .and(paths.deleting().ne(true)?);
    Ok((
        filter.into_document(),
        paths.deleting().set(true)?.into_document(),
    ))
}

/// The path of `file` from the folder of the document at `document`, as a
/// bundle names it, or `None` when `file` is not in that folder.
fn name_in<'file>(document: &StorePath, file: &'file str) -> Option<&'file str> {
    document
        .as_str()
        .rsplit_once('/')
        .map_or(Some(file), |(folder, _)| {
            file.strip_prefix(folder)?.strip_prefix('/')
        })
}

/// The save that `held`, a document's record read by its `save` alone, names.
fn named_by(path: &StorePath, held: Document) -> Result<Replaced, StoreError> {
    let named: HeldSave =
        bson::deserialize_from_document(held).map_err(|error| failed(path, &error))?;
    Ok(named.save.map_or(Replaced::Unnamed, Replaced::Save))
}

/// The path, in the bucket of the store's files, of the object whose bytes
/// have the SHA-256 `hash`: `files/<sha256>`.
///
/// ```rust
/// use arky_store::mongo::object_path;
///
/// let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
/// assert_eq!(
///     object_path(hash)?.as_str(),
///     "files/e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
/// );
/// # Ok::<(), arky_store::StoreError>(())
/// ```
///
/// # Errors
///
/// `StoreError::InvalidPath` when `hash` does not make a store path.
pub fn object_path(hash: &str) -> Result<StorePath, StoreError> {
    StorePath::new(&format!("files/{hash}"))
}

/// `raw` as a `Document`, converted without serde: the driver's serde read of
/// a deep record overflows a 2 MB debug stack.
fn record_of(raw: &bson::RawDocument) -> Result<Document, StoreError> {
    Document::try_from(raw).map_err(|error| backend(&error))
}

/// The typed filter on `path`.
fn on_path(path: &StorePath) -> Result<Document, StoreError> {
    let filter = StoredDocument::MONGO_FIELDS
        .path()
        .eq(path.as_str().to_owned())
        .map_err(|error| failed(path, &error))?;
    Ok(filter.into_document())
}

/// The typed filter on `path` and `revision`.
fn on_revision(path: &StorePath, revision: &Revision) -> Result<Document, StoreError> {
    let paths = StoredDocument::MONGO_FIELDS;
    let filter = paths
        .path()
        .eq(path.as_str().to_owned())
        .and_then(|on_path| Ok(on_path.and(paths.revision().eq(revision.0.clone())?)))
        .map_err(|error| failed(path, &error))?;
    Ok(filter.into_document())
}

/// The failure of a deletion of `object` after the change to its document is
/// stored.
fn stopped_deletion(object: &StorePath, error: &dyn fmt::Display) -> StoreError {
    StoreError::Backend(format!(
        "{object}: the change to the document is stored, and the deletion of this object stopped: {error}; `arky asset finish-deletions` finishes it"
    ))
}

/// The failure to take back the claims of the document at `path` after its
/// change is stored.
fn stored_release_failed(path: &StorePath, error: &dyn fmt::Display) -> StoreError {
    StoreError::Backend(format!(
        "{path}: the change to the document is stored, and taking back the claims on the files it no longer embeds failed: {error}"
    ))
}

#[cfg(test)]
#[cfg(store_tests)]
mod tests;
