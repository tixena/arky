//! The `MongoDB` store a `mongodb:` or `mongodb+srv:` string names:
//! `mongodb[+srv]://[user:password@]host[:port][,host[:port]…]/<database>/<collection>[/<folder>]?files=…[&<driver option>…]`.
//!
//! `files` names the `MinIO` store that keeps the collection's files, as a
//! `MinIO` store string written percent-encoded or as `arky://<name>`; the
//! collection keeps them under `<database>/<collection>` of that store. Every
//! other option goes to the driver as written. A folder after the collection
//! is a folder inside the store.

use std::path::PathBuf;

use mongodb::error::{Error, ErrorKind};
use mongodb::options::ConnectionString;
use url::form_urlencoded;

use super::expand::expand;
use super::minio::{self, Minio};
use super::{Backend, Context, decoded, scheme_of, within};
use crate::lock::LockSettings;
use crate::mongo::MongoStore;
use crate::{BoxFuture, DynStore, StoreError, StorePath};

/// The schemes of a `MinIO` store string, the only store `files` may name.
const MINIO_SCHEMES: &[&str] = &["minio+http", "minio+https"];

/// The schemes of a `MongoDB` store string.
const SCHEMES: &[&str] = &["mongodb", "mongodb+srv"];

/// Where a `MongoDB` store string says the store is.
pub(super) struct Mongo {
    pub collection: String,
    pub database: String,
    /// The connection string the driver gets: the text without the
    /// collection, the folder and `files`.
    pub driver: String,
    pub files: Minio,
    pub folder: Option<StorePath>,
}

/// The `MongoDB` store over `mongodb` and `mongodb+srv`.
#[derive(Debug)]
pub(super) struct MongoBackend;

/// A `MongoDB` store string cut into its parts as written.
struct Parts<'text> {
    authority: &'text str,
    collection: &'text str,
    database: &'text str,
    /// What follows the collection, before the query.
    rest: &'text str,
    scheme: &'text str,
    /// The query and the fragment, from the first `?` or `#`.
    suffix: &'text str,
}

impl Backend for MongoBackend {
    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        Box::pin(opened(text, context))
    }

    fn resolve(&self, _text: &str) -> Option<Result<(String, PathBuf), StoreError>> {
        None
    }

    fn schemes(&self) -> &'static [&'static str] {
        SCHEMES
    }

    /// The collection, `mongodb://host/<database>/<collection>` with the
    /// query, and the path after it.
    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let Parts {
            authority,
            collection,
            database,
            rest,
            scheme,
            suffix,
        } = parts(text)?;
        let container = format!("{scheme}://{authority}/{database}/{collection}{suffix}");
        Ok((container, decoded(rest)?))
    }
}

/// The `MinIO` store `written`, the value of `files` in `text`, names: an
/// `arky://` name resolved to its saved store string, its variables replaced.
fn files_of(text: &str, written: &str, context: Context<'_>) -> Result<Minio, StoreError> {
    let (resolved, _) = context
        .backends
        .resolve(written, context.base)
        .map_err(|error| of_files(text, error))?;
    let scheme = scheme_of(&resolved).map_or("a plain path", |(scheme, _)| scheme);
    if !MINIO_SCHEMES.contains(&scheme) {
        return Err(StoreError::InvalidStore(format!(
            "files names {scheme}; a MongoDB store keeps its files in MinIO only"
        )));
    }
    // Only a saved store string still holds its `${…}`: `text` was expanded.
    let minio = if resolved == written {
        minio::parse(&resolved)
    } else {
        expand(&resolved, context.system, true).and_then(|expanded| minio::parse(expanded.text()))
    };
    minio.map_err(|error| of_files(text, error))
}

/// The value of `pair` percent-decoded when it is the `files` option.
fn files_value(pair: &str) -> Option<String> {
    form_urlencoded::parse(pair.as_bytes())
        .next()
        .filter(|(name, _)| name == "files")
        .map(|(_, value)| value.into_owned())
}

/// The folder `rest`, what follows the collection, names, if any.
fn folder_of(rest: &str) -> Result<Option<StorePath>, StoreError> {
    let folder = rest.trim_matches('/');
    if folder.is_empty() {
        return Ok(None);
    }
    let Ok(inner) = StorePath::new(&decoded(folder)?) else {
        return Err(StoreError::InvalidStore(
            "the folder after the collection is not a store path".to_owned(),
        ));
    };
    Ok(Some(inner))
}

/// `error`, which the store `files` names gave, as an error of `text`: an
/// `InvalidStore` names `files`, a `NotConfigured` names `text` and `files`.
fn of_files(text: &str, error: StoreError) -> StoreError {
    match error {
        StoreError::InvalidStore(message) => StoreError::InvalidStore(format!("files: {message}")),
        StoreError::NotConfigured(message) => {
            StoreError::NotConfigured(format!("`{text}`: files: {message}"))
        }
        StoreError::Backend(_)
        | StoreError::Conflict(_)
        | StoreError::InvalidPath(_)
        | StoreError::NotFound(_)
        | StoreError::Unsupported(_) => error,
    }
}

/// The store `text` names, with the folder written after the collection
/// inside it.
async fn opened<'store>(
    text: &str,
    context: Context<'store>,
) -> Result<Box<dyn DynStore + 'store>, StoreError> {
    let Mongo {
        collection,
        database,
        driver,
        files,
        folder,
    } = parse(text, context)?;
    let bucket = minio::build(files, context.system)
        .map_err(|error| of_files(text, error))?
        .locking(LockSettings::default());
    let store = MongoStore::open(&driver, &database, &collection, bucket).await?;
    Ok(placed(Box::new(store), folder))
}

/// Where the `MongoDB` store string `text` says the store is, its `files`
/// resolved through `context`.
///
/// The driver parses its connection string first; a refusal names the option
/// or the part it refuses, never the string.
pub(super) fn parse(text: &str, context: Context<'_>) -> Result<Mongo, StoreError> {
    let Parts {
        authority,
        collection,
        database,
        rest,
        scheme,
        suffix,
    } = parts(text)?;
    if suffix.contains('#') {
        return Err(StoreError::InvalidStore(
            "a fragment, which a store string does not take".to_owned(),
        ));
    }
    let query = suffix.strip_prefix('?').unwrap_or(suffix);
    let mut files = None;
    let mut options = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let Some(value) = files_value(pair) else {
            options.push(pair);
            continue;
        };
        if files.replace(value).is_some() {
            return Err(StoreError::InvalidStore("files is given twice".to_owned()));
        }
    }
    let Some(written) = files.filter(|value| !value.is_empty()) else {
        return Err(StoreError::InvalidStore(
            "files is required: the MinIO store of the collection's files".to_owned(),
        ));
    };
    let driver = if options.is_empty() {
        format!("{scheme}://{authority}/{database}")
    } else {
        format!("{scheme}://{authority}/{database}?{}", options.join("&"))
    };
    ConnectionString::parse(&driver).map_err(|error| refused(&error))?;
    let database_name = decoded(database)?;
    let collection_name = decoded(collection)?;
    if StorePath::new(&format!("{database_name}/{collection_name}")).is_err() {
        return Err(StoreError::InvalidStore(
            "the database and the collection do not make a store path".to_owned(),
        ));
    }
    Ok(Mongo {
        collection: collection_name,
        database: database_name,
        driver,
        files: files_of(text, &written, context)?,
        folder: folder_of(rest)?,
    })
}

/// `text` cut into its parts as written. The host list is copied as text, as
/// a URL parser does not read `host1:27017,host2:27017`.
fn parts(text: &str) -> Result<Parts<'_>, StoreError> {
    let Some((scheme, after)) = text
        .split_once("://")
        .filter(|(scheme, _)| SCHEMES.contains(scheme))
    else {
        return Err(StoreError::InvalidStore(
            "a MongoDB store string starts with mongodb:// or mongodb+srv://".to_owned(),
        ));
    };
    let (written, suffix) = after.split_at(after.find(['?', '#']).unwrap_or(after.len()));
    let (authority, path) = written.split_once('/').unwrap_or((written, ""));
    let mut segments = path.splitn(3, '/');
    let database = segments.next().unwrap_or_default();
    if database.is_empty() {
        return Err(StoreError::InvalidStore(
            "no database: the path is /<database>/<collection>".to_owned(),
        ));
    }
    let collection = segments.next().unwrap_or_default();
    if collection.is_empty() {
        return Err(StoreError::InvalidStore(
            "no collection: the path is /<database>/<collection>".to_owned(),
        ));
    }
    Ok(Parts {
        authority,
        collection,
        database,
        rest: segments.next().unwrap_or_default(),
        scheme,
        suffix,
    })
}

/// `store`, with `folder` inside it when the string names one.
pub(super) fn placed<'store>(
    store: Box<dyn DynStore + 'store>,
    folder: Option<StorePath>,
) -> Box<dyn DynStore + 'store> {
    let Some(inner) = folder else {
        return store;
    };
    within(store, inner)
}

/// The driver's refusal of its connection string, naming the option or the
/// part it refuses.
fn refused(error: &Error) -> StoreError {
    let message = if let ErrorKind::InvalidArgument { message, .. } = error.kind.as_ref() {
        message.clone()
    } else {
        error.kind.to_string()
    };
    StoreError::InvalidStore(message)
}
