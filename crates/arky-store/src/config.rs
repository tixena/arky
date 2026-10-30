//! The configuration arky reads from `arky.toml` files: the saved stores, the
//! default store and the lock.
//!
//! ```toml
//! default = "personal_notes"
//!
//! [lock]
//! expires_seconds = 60
//! wait_seconds = 2
//!
//! [stores.personal_notes]
//! url = "file:notes"
//! ```
//!
//! [`Configuration::discover`] reads every `arky.toml` from a folder up to
//! the root of the file system: a store name, `default` and each key of
//! `[lock]` come from the nearest file that sets them. [`Configuration::read`]
//! reads one named file. Other top-level keys are left to other tools.

use alloc::collections::BTreeMap;
use core::fmt::Display;
use core::time::Duration;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use arky::parser::ast::{Mapping, Value};
use arky::parser::read_toml;
use os_shim::System;

use crate::StoreError;
use crate::lock::LockSettings;

/// The scheme of a saved store's name, which a saved store's `url` cannot
/// take.
const ARKY_SCHEME: &str = "arky://";

/// The name of the configuration file in each folder.
const FILE_NAME: &str = "arky.toml";

/// The configuration found in `arky.toml` files: the saved stores, the
/// default store and the lock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Configuration {
    /// The saved store used when none is named, by name.
    pub default: Option<String>,
    /// The lock the file store and the bucket stores hold during a save.
    pub lock: LockSettings,
    /// The saved stores, by name.
    pub stores: SavedStores,
}

impl Configuration {
    /// Every `arky.toml` from `folder` up to the root of the file system,
    /// merged: a store name, `default` and each key of `[lock]` come from the
    /// nearest file that sets them.
    ///
    /// A relative `folder` is taken against the current folder. A folder with
    /// no `arky.toml` adds nothing; with none anywhere, this is
    /// `Configuration::default()`.
    ///
    /// # Errors
    ///
    /// `StoreError::NotConfigured`, naming the file, when an `arky.toml` does
    /// not read or one of its keys breaks a rule.
    pub fn discover(system: &dyn System, folder: &Path) -> Result<Self, StoreError> {
        let start = resolved(&against_current(system, folder)?);
        let mut found = Found::default();
        for ancestor in start.ancestors() {
            let file = ancestor.join(FILE_NAME);
            if let Some(source) = source_of(system, &file, &file)? {
                found.fill_from(parse(&file, &file, &source)?);
            }
        }
        Ok(found.into_configuration())
    }

    /// The one file `file`, as `--config` and `ARKY_CONFIG` name it.
    ///
    /// A relative `file` is taken against the current folder. No other file
    /// is read.
    ///
    /// # Errors
    ///
    /// `StoreError::NotConfigured`, naming `file`, when it does not exist,
    /// does not read, or one of its keys breaks a rule.
    pub fn read(system: &dyn System, file: &Path) -> Result<Self, StoreError> {
        let path = against_current(system, file)?;
        let source = source_of(system, &path, file)?
            .ok_or_else(|| in_file(file, &"the configuration file does not exist"))?;
        Ok(parse(file, &path, &source)?.into_configuration())
    }
}

/// A store saved under a name in an `arky.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SavedStore {
    /// The `arky.toml` that defines it.
    pub defined_in: PathBuf,
    /// The name, the key of its `[stores.<name>]` table.
    pub name: String,
    /// The store string as written, its `${…}` variables unexpanded.
    pub url: String,
}

/// The saved stores in effect, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedStores(BTreeMap<String, SavedStore>);

impl SavedStores {
    /// The saved stores in effect from `folder`:
    /// `Configuration::discover(…)?.stores`.
    ///
    /// # Errors
    ///
    /// The errors of [`Configuration::discover`].
    pub fn discover(system: &dyn System, folder: &Path) -> Result<Self, StoreError> {
        Ok(Configuration::discover(system, folder)?.stores)
    }

    /// The store saved as `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SavedStore> {
        self.0.get(name)
    }

    /// Every store, sorted by name.
    pub fn list(&self) -> impl Iterator<Item = &SavedStore> {
        self.0.values()
    }
}

/// What the `arky.toml` files read so far set.
#[derive(Default)]
struct Found {
    default: Option<String>,
    expires: Option<Duration>,
    stores: BTreeMap<String, SavedStore>,
    wait: Option<Duration>,
}

impl Found {
    /// Takes from `farther`, a file farther up the tree, what `self` leaves
    /// unset: a store name whole, `default`, and each key of `[lock]`.
    fn fill_from(&mut self, farther: Self) {
        self.default = self.default.take().or(farther.default);
        self.expires = self.expires.or(farther.expires);
        self.wait = self.wait.or(farther.wait);
        for (name, store) in farther.stores {
            self.stores.entry(name).or_insert(store);
        }
    }

    /// The configuration, with the usual lock for a key no file sets.
    fn into_configuration(self) -> Configuration {
        let usual = LockSettings::default();
        Configuration {
            default: self.default,
            lock: LockSettings::new(
                self.expires.unwrap_or(usual.expires),
                self.wait.unwrap_or(usual.wait),
            ),
            stores: SavedStores(self.stores),
        }
    }
}

/// `path` against the current folder when it is relative.
fn against_current(system: &dyn System, path: &Path) -> Result<PathBuf, StoreError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    system
        .current_dir()
        .map(|folder| folder.join(path))
        .map_err(|error| in_file(path, &format!("the current folder: {error}")))
}

/// `what` went wrong with the configuration file `file`.
fn in_file(file: &Path, what: &dyn Display) -> StoreError {
    StoreError::NotConfigured(format!("{}: {what}", file.display()))
}

/// The settings of `[lock]`, the table `lock` of `file`, into `found`.
fn lock_table(file: &Path, lock: &Mapping, found: &mut Found) -> Result<(), StoreError> {
    for (key, value) in lock {
        let seconds = match (key.as_str(), value) {
            ("expires_seconds" | "wait_seconds", Value::Integer(whole)) if *whole > 0 => {
                Duration::from_secs(whole.unsigned_abs())
            }
            ("expires_seconds" | "wait_seconds", _) => {
                return Err(in_file(
                    file,
                    &format!("{key} in [lock] is not a whole number of seconds above zero"),
                ));
            }
            _ => {
                return Err(in_file(
                    file,
                    &format!("[lock] has no key {key}; it takes expires_seconds and wait_seconds"),
                ));
            }
        };
        if key == "expires_seconds" {
            found.expires = Some(seconds);
        } else {
            found.wait = Some(seconds);
        }
    }
    Ok(())
}

/// What the text `source` of the configuration file `file` sets, its stores
/// defined in `defined_in`.
fn parse(file: &Path, defined_in: &Path, source: &str) -> Result<Found, StoreError> {
    let tables = read_toml(source, 0..source.len()).map_err(|error| in_file(file, &error))?;
    let mut found = Found::default();
    match tables.get("default") {
        None => {}
        Some(Value::String(name)) => found.default = Some(name.clone()),
        Some(_) => {
            return Err(in_file(
                file,
                &"default is not a string; it names a saved store",
            ));
        }
    }
    match tables.get("lock") {
        None => {}
        Some(Value::Mapping(lock)) => lock_table(file, lock, &mut found)?,
        Some(_) => return Err(in_file(file, &"lock is not a table")),
    }
    match tables.get("stores") {
        None => {}
        Some(Value::Mapping(stores)) => {
            for (name, table) in stores {
                let store = saved_store(file, defined_in, name, table)?;
                found.stores.insert(name.clone(), store);
            }
        }
        Some(_) => return Err(in_file(file, &"stores is not a table")),
    }
    Ok(found)
}

/// `path` with `.` and `..` resolved from its text, so that its ancestors are
/// the folders above the one it names.
fn resolved(path: &Path) -> PathBuf {
    let mut folder = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                folder.pop();
            }
            Component::Normal(_) | Component::Prefix(_) | Component::RootDir => {
                folder.push(component);
            }
        }
    }
    folder
}

/// The store `[stores.<name>]`, the value `table` of `file`, defined in
/// `defined_in`.
fn saved_store(
    file: &Path,
    defined_in: &Path,
    name: &str,
    table: &Value,
) -> Result<SavedStore, StoreError> {
    let Value::Mapping(keys) = table else {
        return Err(in_file(file, &format!("[stores.{name}] is not a table")));
    };
    let mut written = None;
    for (key, value) in keys {
        match (key.as_str(), value) {
            ("url", Value::String(text)) => written = Some(text),
            ("url", _) => {
                return Err(in_file(
                    file,
                    &format!("[stores.{name}] url is not a string"),
                ));
            }
            _ => {
                return Err(in_file(
                    file,
                    &format!("[stores.{name}] has no key {key}; it takes url"),
                ));
            }
        }
    }
    let Some(url) = written else {
        return Err(in_file(file, &format!("[stores.{name}] has no url")));
    };
    // A saved store names no other, so a name resolves in one step.
    let nested = url
        .get(..ARKY_SCHEME.len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case(ARKY_SCHEME));
    if nested {
        return Err(in_file(
            file,
            &format!(
                "[stores.{name}] url names another saved store; a saved store's url is any store string but arky://"
            ),
        ));
    }
    Ok(SavedStore {
        defined_in: defined_in.to_path_buf(),
        name: name.to_owned(),
        url: url.clone(),
    })
}

/// The text of the configuration file at `path`, or `None` where no file is;
/// an error names `file`.
fn source_of(system: &dyn System, path: &Path, file: &Path) -> Result<Option<String>, StoreError> {
    match system.read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(in_file(file, &error)),
    }
}

#[cfg(test)]
mod tests;
