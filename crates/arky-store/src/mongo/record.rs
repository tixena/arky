//! One file as one MongoDB record, and the object of each file it embeds as
//! another.
//!
//! The frontmatter is stored as plain BSON values, dates as BSON dates, so a
//! query can name a frontmatter key.

use alloc::collections::BTreeMap;
use core::iter;

use arky_core::parser::ast::{Mapping, RawDocument, Value, raw_document_schema};
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
    pub document: String,
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
    #[serde(rename = "doc")]
    pub document: RawDocument,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub path: String,
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
    pub content_type: String,
    #[serde(default)]
    pub deleting: bool,
    pub description: String,
    pub embedded_by: Vec<Claim>,
    #[serde(rename = "_id")]
    pub id: String,
    pub object: String,
    pub size: u64,
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
