//! The store an `arky://<name>` string names: the store saved as
//! `[stores.<name>]` in the `arky.toml` files.

use std::path::{Path, PathBuf};

use url::form_urlencoded;

use super::{Backend, Context, decoded, within};
use crate::config::{SavedStore, SavedStores};
use crate::{BoxFuture, DynStore, StoreError, StorePath};

/// The scheme and its slashes.
const SCHEME: &str = "arky://";

/// The backend of `arky://<name>`: the stores saved in `arky.toml`.
///
/// `arky://<name>` opens the saved store string of `<name>`, its `${…}`
/// variables replaced and a relative path taken against the folder of the
/// `arky.toml` that saves it. In a path argument, `arky://<name>/<path>` is
/// that store and the path inside it; opened as a store,
/// `arky://<name>/<folder>` is that store with the folder inside it. The
/// backend takes no option. An error in the saved store string names both
/// the `arky://` text and the saved store string as written, with its file.
///
/// ```rust
/// use std::path::Path;
///
/// use arky_store::backend::Backends;
/// use arky_store::backend::arky::SavedBackend;
/// use arky_store::config::Configuration;
/// use arky_store::{Precondition, StorePath};
/// use os_shim::System as _;
/// use os_shim::mock::MemorySystem;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let saved = b"[stores.personal_notes]\nurl = \"file:notes\"\n";
///     let system = MemorySystem::new().with_file("/home/me/arky.toml", saved)?;
///     let current_folder = Path::new("/home/me/work");
///     let configuration = Configuration::discover(&system, current_folder)?;
///     let backends = Backends::default().with(SavedBackend::new(configuration.stores.clone()));
///     let lock = configuration.lock;
///
///     let store = backends.open("arky://personal_notes", current_folder, lock, &system).await?;
///     store.write(&StorePath::new("a.md")?, b"# A\n", &Precondition::Absent).await?;
///     assert_eq!(system.read_to_string(Path::new("/home/me/notes/a.md"))?, "# A\n");
///
///     let in_path = "arky://personal_notes/ideas/today.md";
///     let (_, at) = backends.open_path(in_path, current_folder, lock, &system).await?.ok_or("a store string")?;
///     assert_eq!(at.as_str(), "ideas/today.md");
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct SavedBackend {
    stores: SavedStores,
}

/// The saved store an `arky://` string names, and the folder written after
/// its name.
struct Named<'stores> {
    folder: Option<StorePath>,
    saved: &'stores SavedStore,
}

impl SavedBackend {
    /// The saved store `text` names, and the folder written after its name.
    fn named(&self, text: &str) -> Result<Named<'_>, StoreError> {
        let (name, folder) =
            parse(text).map_err(|error| StoreError::InvalidStore(format!("`{text}`: {error}")))?;
        let Some(saved) = self.stores.get(&name) else {
            return Err(StoreError::NotConfigured(format!(
                "`{text}`: no store named {name} in any arky.toml"
            )));
        };
        Ok(Named { folder, saved })
    }

    /// A backend over `stores`.
    #[must_use]
    pub const fn new(stores: SavedStores) -> Self {
        Self { stores }
    }

    /// The store `text` names, with the folder written after the name inside
    /// it.
    async fn opened<'store>(
        &'store self,
        text: &str,
        context: Context<'store>,
    ) -> Result<Box<dyn DynStore + 'store>, StoreError> {
        let Named { folder, saved } = self.named(text)?;
        let store = context
            .backends
            .open(&saved.url, folder_of(saved), context.lock, context.system)
            .await
            .map_err(|error| inside(text, saved, error))?;
        let Some(inner) = folder else {
            return Ok(store);
        };
        Ok(within(store, inner))
    }
}

impl Backend for SavedBackend {
    fn nested_options(&self) -> &'static [&'static str] {
        &[]
    }

    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        Box::pin(self.opened(text, context))
    }

    /// The saved store string of `arky://<name>` and the folder of the
    /// `arky.toml` that saves it; a folder after the name is refused.
    fn resolve(&self, text: &str) -> Option<Result<(String, PathBuf), StoreError>> {
        Some(self.named(text).and_then(|named| {
            if named.folder.is_some() {
                return Err(StoreError::InvalidStore(format!(
                    "`{text}`: a folder after the store name, where only a store is named"
                )));
            }
            Ok((
                named.saved.url.clone(),
                folder_of(named.saved).to_path_buf(),
            ))
        }))
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["arky"]
    }

    /// The saved store, `arky://<name>` with the query, and the path after
    /// it.
    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let after = after_scheme(text)?;
        let (written, suffix) = after.split_at(after.find(['?', '#']).unwrap_or(after.len()));
        let (name, rest) = written.split_once('/').unwrap_or((written, ""));
        Ok((format!("{SCHEME}{name}{suffix}"), decoded(rest)?))
    }
}

/// What `text` writes after `arky://`.
fn after_scheme(text: &str) -> Result<&str, StoreError> {
    text.strip_prefix(SCHEME).ok_or_else(|| {
        StoreError::InvalidStore("a saved store string starts with arky://".to_owned())
    })
}

/// The folder of the `arky.toml` that saves `saved`: its relative paths are
/// relative to it.
fn folder_of(saved: &SavedStore) -> &Path {
    saved.defined_in.parent().unwrap_or(&saved.defined_in)
}

/// `error`, which the store string `saved` saves gave, naming `text` and that
/// store string as written with the file that saves it.
fn inside(text: &str, saved: &SavedStore, error: StoreError) -> StoreError {
    let url = format!("`{}`", saved.url);
    let both = format!("`{text}` ({url} in {})", saved.defined_in.display());
    let named = |message: &str| {
        if message.contains(&url) {
            message.replacen(&url, &both, 1)
        } else {
            format!("{both}: {message}")
        }
    };
    match error {
        StoreError::InvalidStore(message) => StoreError::InvalidStore(named(&message)),
        StoreError::NotConfigured(message) => StoreError::NotConfigured(named(&message)),
        StoreError::Backend(_)
        | StoreError::Conflict(_)
        | StoreError::InvalidPath(_)
        | StoreError::NotFound(_)
        | StoreError::Unsupported(_) => error,
    }
}

/// The name `text` writes after `arky://`, percent-decoded, and the folder
/// after it, if any. An option and a fragment are refused.
fn parse(text: &str) -> Result<(String, Option<StorePath>), StoreError> {
    let after = after_scheme(text)?;
    let (before, fragment) = after
        .split_once('#')
        .map_or((after, None), |(before, fragment)| (before, Some(fragment)));
    let (written, query) = before.split_once('?').unwrap_or((before, ""));
    if let Some((option, _)) = form_urlencoded::parse(query.as_bytes()).next() {
        return Err(StoreError::InvalidStore(format!("unknown option {option}")));
    }
    if fragment.is_some() {
        return Err(StoreError::InvalidStore(
            "a fragment, which a store string does not take".to_owned(),
        ));
    }
    let (name, rest) = written.split_once('/').unwrap_or((written, ""));
    if name.is_empty() {
        return Err(StoreError::InvalidStore("no store name".to_owned()));
    }
    let folder = rest.trim_matches('/');
    if folder.is_empty() {
        return Ok((decoded(name)?, None));
    }
    let Ok(inner) = StorePath::new(&decoded(folder)?) else {
        return Err(StoreError::InvalidStore(
            "the folder after the store name is not a store path".to_owned(),
        ));
    };
    Ok((decoded(name)?, Some(inner)))
}
