//! One file as one MongoDB record, and the object of each file it embeds as
//! another.
//!
//! The frontmatter is stored as plain BSON values, dates as BSON dates, so a
//! query can name a frontmatter key.
//!
//! A date is stored as a BSON date at midnight UTC, and a date-time as its
//! instant in UTC. A time of day is stored as `{ t: "time", v: "07:32:00" }`,
//! and a float that is not finite as the text `nan`, `inf` or `-inf`.
//!
//! [`StoredDocument`] is the record of a document, [`StoredObject`] the record
//! of an embedded file's object, and [`Claim`] a document's claim on that
//! object. [`to_record`], [`from_record`], [`frontmatter_is`] and
//! [`revision_of`] need no server.
//!
//! Each record type also carries the `MongoDB` layer tixschema generates:
//! `MONGO_FIELDS`, whose typed paths build `bson` filters and updates, and
//! typed operations such as `find` over a `mongodb::Collection` of
//! `bson::Document`. The modules `claim_schema`, `stored_document_schema` and
//! `stored_object_schema` hold the filter, update and error types those
//! name.

use alloc::collections::BTreeMap;
use core::iter;

use arky::parser::ast::{Mapping, RawDocument, Value, raw_document_schema};
use bson::oid::ObjectId;
use bson::{Bson, Document, doc};
use serde::{Deserialize, Serialize};
use tixschema::model_schema;

use crate::{Revision, StoreError, file};

/// The deepest record MongoDB holds, each document and each array one level.
const MAX_DEPTH: usize = 100;

/// The revision of a path that holds no document: no revision `revision_of`
/// gives is equal to it.
pub const NO_DOCUMENT: &str = "none";

/// A document's claim on an object of the store's files: the document's path,
/// the path its embed names, and the save that made the claim.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Claim {
    /// The store path of the document that embeds the file.
    pub document: String,
    /// The store path of the embedded file, beside the document.
    pub path: String,
    /// The save that made the claim: `None` for a claim written before claims
    /// named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<ObjectId>,
}

/// One file of a MongoDB store: its path, its revision, its document, the
/// SHA-256 of each file it embeds, by the file's path in the bundle, and the
/// save that wrote it.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StoredDocument {
    /// The parsed document, stored under `doc`. Its frontmatter is `doc.data`.
    #[serde(rename = "doc")]
    pub document: RawDocument,
    /// The SHA-256 of each file the document embeds, by the file's path in
    /// the bundle.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// The record's `_id`.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    /// The document's store path, unique in the collection.
    pub path: String,
    /// The document's revision: what `revision_of` gives for its text in
    /// arky's form and the SHA-256 of each file it embeds.
    pub revision: String,
    /// The save that wrote the record: `None` for a record written before
    /// records named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<ObjectId>,
}

/// The record of one object of a MongoDB store's files, in
/// `<collection>_files`, keyed by the SHA-256 of its bytes.
///
/// It holds the object's key in the bucket, its media type, size and
/// description, whether it is in the bucket, whether a deletion holds it, and
/// each document that embeds it.
#[model_schema(enable = ["decode_with", "mongodb"])]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StoredObject {
    /// The file's media type, from its extension, such as `image/png`.
    pub content_type: String,
    /// Whether a deletion holds the record.
    #[serde(default)]
    pub deleting: bool,
    /// The alt text of the first embed that names the file.
    pub description: String,
    /// Each document's claim on the object. The object is deleted once none
    /// is left.
    pub embedded_by: Vec<Claim>,
    /// The SHA-256 of the object's bytes, stored as `_id`.
    #[serde(rename = "_id")]
    pub id: String,
    /// The object's path in the store of the files: `files/<sha256>`.
    pub object: String,
    /// The file's size in bytes.
    pub size: u64,
    /// Whether the object is in the bucket.
    pub stored: bool,
}

/// The record sent to MongoDB: serde's BSON of `stored`.
///
/// # Errors
///
/// `StoreError::Backend` naming the path when the record does not serialize,
/// or nests deeper than the 100 levels MongoDB holds.
pub fn to_record(stored: &StoredDocument) -> Result<Document, StoreError> {
    let too_deep = |levels: usize| {
        StoreError::Backend(format!(
            "{} nests {levels} levels, and MongoDB holds {MAX_DEPTH}",
            stored.path
        ))
    };
    let frontmatter = if let RawDocument::Valid(document) = &stored.document {
        document.frontmatter.as_ref()
    } else {
        None
    };
    // serde writes a deep frontmatter by recursion, so it is measured first.
    if let Some(levels) = frontmatter
        .map(frontmatter_depth)
        .filter(|levels| *levels > MAX_DEPTH)
    {
        return Err(too_deep(levels));
    }
    let record = bson::serialize_to_document(stored)
        .map_err(|error| StoreError::Backend(format!("{}: {error}", stored.path)))?;
    let levels = depth(&record);
    if levels > MAX_DEPTH {
        return Err(too_deep(levels));
    }
    Ok(record)
}

/// A record read back with plain serde.
///
/// A record with no `files` reads with none, and one with no `save` reads with
/// `None`.
///
/// ```rust
/// use arky::parser::parse;
/// use arky_store::mongo::record::{from_record, to_record};
/// use bson::doc;
/// use bson::oid::ObjectId;
///
/// let record = doc! {
///     "doc": bson::serialize_to_bson(&parse("# Groceries\n").document)?,
///     "_id": ObjectId::parse_str("6ad42f681472f05ab2ed9be7")?,
///     "path": "lists/groceries.md",
///     "revision": "sha256:0",
/// };
/// let stored = from_record(record)?;
/// assert_eq!(stored.path, "lists/groceries.md");
/// assert!(stored.files.is_empty() && stored.save.is_none());
/// assert_eq!(to_record(&stored)?.get_str("path")?, "lists/groceries.md");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
///
/// `StoreError::Backend` naming the record's `_id` and serde's first error.
pub fn from_record(record: Document) -> Result<StoredDocument, StoreError> {
    let id = record
        .get("_id")
        .map_or_else(|| "with no `_id`".to_owned(), ToString::to_string);
    bson::deserialize_from_document(record)
        .map_err(|error| StoreError::Backend(format!("the record {id} does not read: {error}")))
}

/// The filter on the frontmatter key `key`, the value as `to_record` stores
/// it: `{ "doc.data.<key>": value }`, or a `$getField` match for a key a
/// dotted path cannot name, one holding a `.` or starting with `$`.
///
/// ```rust
/// use arky::parser::ast::Value;
/// use arky_store::mongo::record::frontmatter_is;
/// use bson::doc;
///
/// let open = frontmatter_is("status", &Value::String("open".to_owned()))?;
/// assert_eq!(open.to_string(), r#"{ "doc.data.status": "open" }"#);
///
/// let dotted = frontmatter_is("v1.2", &Value::Integer(3))?;
/// let by_name = doc! { "$expr": { "$eq": [
///     { "$getField": { "field": { "$literal": "v1.2" }, "input": "$doc.data" } },
///     3_i64,
/// ] } };
/// assert_eq!(dotted, by_name);
/// # Ok::<(), arky_store::StoreError>(())
/// ```
///
/// # Errors
///
/// `StoreError::Backend` naming the key when the value does not serialize.
pub fn frontmatter_is(key: &str, value: &Value) -> Result<Document, StoreError> {
    let stored = bson::serialize_to_bson(value)
        .map_err(|error| StoreError::Backend(format!("{key}: {error}")))?;
    if key.contains('.') || key.starts_with('$') {
        return Ok(doc! { "$expr": { "$eq": [
            { "$getField": { "field": { "$literal": key }, "input": "$doc.data" } },
            stored,
        ] } });
    }
    let mut filter = Document::new();
    filter.insert(format!("doc.data.{key}"), stored);
    Ok(filter)
}

/// `sha256:` and the SHA-256 of `text` and of each file of `files`, its path
/// in the bundle and the SHA-256 of its bytes, each part preceded by its
/// length in bytes, in decimal, and a `:`.
///
/// ```rust
/// use std::collections::BTreeMap;
///
/// use arky_store::mongo::record::revision_of;
///
/// assert_eq!(
///     revision_of("", &BTreeMap::new()).0,
///     "sha256:ba768b331fd86cec803be04e56ab2b3d4c0e98ef4ee4fcd4e72ad7cce61a1d1f",
/// );
/// let logo = BTreeMap::from([("media/logo.png".to_owned(), "abc".to_owned())]);
/// assert_eq!(
///     revision_of("# Report\n", &logo).0,
///     "sha256:b3dba8daff13fe05db0539675fb03aa88556e0d780a5c274135db17d8699c755",
/// );
/// ```
#[must_use]
pub fn revision_of(text: &str, files: &BTreeMap<String, String>) -> Revision {
    let mut hashed = String::new();
    let parts = iter::once(text).chain(
        files
            .iter()
            .flat_map(|(path, hash)| [path.as_str(), hash.as_str()]),
    );
    for part in parts {
        hashed.push_str(&part.len().to_string());
        hashed.push(':');
        hashed.push_str(part);
    }
    file::revision(hashed.as_bytes())
}

/// How deep `record` nests: the record is the first level, and each document
/// and each array in it one more. Counted from a list, not by recursion.
fn depth(record: &Document) -> usize {
    let mut deepest = 1_usize;
    let mut open: Vec<(usize, &Bson)> = record.values().map(|value| (2, value)).collect();
    while let Some((level, value)) = open.pop() {
        if let Some(document) = value.as_document() {
            deepest = deepest.max(level);
            open.extend(document.values().map(|inner| (level + 1, inner)));
        }
        if let Some(items) = value.as_array() {
            deepest = deepest.max(level);
            open.extend(items.iter().map(|inner| (level + 1, inner)));
        }
    }
    deepest
}

/// How deep a record nests through `frontmatter` as its `doc.data`, counted as
/// `depth` counts: `data` is level 3, and each mapping, list and time inside
/// it, each stored as a document or an array, one more.
fn frontmatter_depth(frontmatter: &Mapping) -> usize {
    let mut deepest = 3_usize;
    let mut open: Vec<(usize, &Value)> = frontmatter.values().map(|value| (4, value)).collect();
    while let Some((level, value)) = open.pop() {
        if let Value::Mapping(inner) = value {
            deepest = deepest.max(level);
            open.extend(inner.values().map(|item| (level + 1, item)));
        }
        if let Value::Sequence(items) = value {
            deepest = deepest.max(level);
            open.extend(items.iter().map(|item| (level + 1, item)));
        }
        if matches!(value, Value::Time(_)) {
            deepest = deepest.max(level);
        }
    }
    deepest
}

#[cfg(test)]
mod tests;
