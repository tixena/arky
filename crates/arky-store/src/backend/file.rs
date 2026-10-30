//! The file store a `file:` string or a plain path names.
//!
//! A `file:` string is read here and never as a URL, which has no relative
//! path: `file:./notes` is `notes` under the base folder.

use core::future;
use std::path::{Path, PathBuf};

use url::form_urlencoded;

use super::{Backend, Context, decoded};
use crate::config::resolved;
use crate::file::FileStore;
use crate::{BoxFuture, DynStore, StoreError};

/// The scheme and its colon.
const SCHEME: &str = "file:";

/// The file store: `file:///srv/notes`, `file:notes`, and every plain path.
#[derive(Debug)]
pub(super) struct FileBackend;

impl Backend for FileBackend {
    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        Box::pin(future::ready(
            root(text, context.base).map(|folder| store(folder, context)),
        ))
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["file"]
    }

    /// The root, `file:///` for an absolute path and `file:.` for a relative
    /// one, and the path after it.
    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let (path, query) = written(text)?;
        let full = decoded(path)?;
        let (container, rest) = full.strip_prefix('/').map_or_else(
            || ("file:.", trim_current(&full)),
            |absolute| ("file:///", absolute.trim_start_matches('/')),
        );
        let kept = query.map_or_else(
            || container.to_owned(),
            |options| format!("{container}?{options}"),
        );
        Ok((kept, rest.to_owned()))
    }
}

/// The file store at the plain path `path`, taken as written: a relative one
/// against `context.base`.
pub(super) fn plain<'store>(path: &str, context: Context<'store>) -> Box<dyn DynStore + 'store> {
    store(resolved(&context.base.join(path)), context)
}

/// The root folder the `file:` string `text` names: after `//` an absolute
/// path, else a path against `base`, percent-decoded.
fn root(text: &str, base: &Path) -> Result<PathBuf, StoreError> {
    let (path, query) = written(text)?;
    if let Some(options) = query
        && let Some((option, _)) = form_urlencoded::parse(options.as_bytes()).next()
    {
        return Err(StoreError::InvalidStore(format!("unknown option {option}")));
    }
    let (from, relative) = path
        .strip_prefix("//")
        .map_or((base, path), |absolute| (Path::new("/"), absolute));
    Ok(resolved(&from.join(decoded(relative)?)))
}

/// The file store at `root`, holding the context's lock.
fn store<'store>(root: PathBuf, context: Context<'store>) -> Box<dyn DynStore + 'store> {
    Box::new(FileStore::new(root, context.system).locking(context.lock))
}

/// `path` without the `./` parts it starts with.
fn trim_current(path: &str) -> &str {
    let mut rest = path;
    while let Some(after) = rest.strip_prefix("./") {
        rest = after.trim_start_matches('/');
    }
    rest
}

/// The path the `file:` string `text` writes and its query, if any.
fn written(text: &str) -> Result<(&str, Option<&str>), StoreError> {
    let Some(after) = text.strip_prefix(SCHEME) else {
        return Err(StoreError::InvalidStore(
            "a file store string starts with file:".to_owned(),
        ));
    };
    Ok(after
        .split_once('?')
        .map_or((after, None), |(path, query)| (path, Some(query))))
}
