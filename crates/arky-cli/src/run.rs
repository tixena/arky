//! The commands, run over a system and the three standard streams.

use core::fmt::Display;
use std::env::VarError;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use arky::bundle::Missing;
use arky::forms::{Form, encode};
use arky::writer::WriteOptions;
use arky_store::backend::Backends;
use arky_store::backend::arky::SavedBackend;
use arky_store::config::Configuration;
use arky_store::document::{self, DocumentError};
use arky_store::file::FileStore;
use arky_store::lock::LockSettings;
use arky_store::{DynStore, Precondition, Revision, StoreError, StorePath};
use os_shim::System;

use crate::cli::{AssetCommand, Cli, Command, FormatArg, StoreArg, SyntaxArg};

/// The variable naming the configuration file when `--config` names none.
const CONFIG_VARIABLE: &str = "ARKY_CONFIG";

/// The short forms that named a store before store strings, refused rather
/// than read as folders: each one's scheme, its kind of store, and the store
/// string of such a store.
const SHORT_FORMS: [(&str, &str, &str); 3] = [
    ("minio", "a MinIO store", "minio+http://<host>/<bucket>"),
    (
        "mongodb",
        "a MongoDB store",
        "mongodb://<host>/<database>/<collection>?files=<MinIO store>",
    ),
    ("s3", "an S3 store", "s3://<bucket>?region=<region>"),
];

/// The variable naming the store of a get or a write when `--store` names
/// none.
const STORE_VARIABLE: &str = "ARKY_STORE";

/// Why a command stopped: the line after `arky: `, and the exit code.
struct Failure {
    code: u8,
    message: String,
}

/// The store a get or a write works on, the path in it, and its name in
/// messages: `None` for the file system, which messages name by the path.
struct Located<'store> {
    at: StorePath,
    name: Option<Named>,
    store: Box<dyn DynStore + 'store>,
}

/// The text of a store and how messages name it: the text in backticks, or
/// the variable that holds it, as no message shows a variable's value.
struct Named {
    label: String,
    text: String,
}

/// What `--format`, `--frontmatter` and `--allow-missing` ask for: the form,
/// the options the document is written with, and what an embed with no file
/// does; and the lock the configuration sets.
struct Settings {
    form: Form,
    lock: LockSettings,
    missing: Missing,
    options: WriteOptions,
}

/// What every command works over: the system, the current folder, the
/// configuration, and the backends that open a store string.
struct Setup<'system> {
    backends: Backends,
    configuration: Configuration,
    current: PathBuf,
    system: &'system dyn System,
}

impl Failure {
    const fn configuration(message: String) -> Self {
        Self { code: 3, message }
    }

    /// `asset finish-deletions` given `name`, a store that keeps no deletions.
    fn keeps_no_deletions(name: &Named) -> Self {
        Self {
            code: 2,
            message: format!(
                "{} keeps no deletions: asset finish-deletions takes a MongoDB store",
                name.label
            ),
        }
    }

    /// `fix` given `name`, a store that keeps no locks.
    fn keeps_no_locks(name: &Named) -> Self {
        Self {
            code: 2,
            message: format!(
                "{} keeps no locks: fix takes the file system, or a file, MinIO or S3 store",
                name.label
            ),
        }
    }

    /// `error` on one line naming `path` as the command line gave it, in place
    /// of the store path `at` that the error's message opens with.
    fn naming(path: &Path, at: Option<&StorePath>, code: u8, error: &dyn Display) -> Self {
        let text = error.to_string();
        let what = at
            .and_then(|place| text.strip_prefix(place.as_str()))
            .and_then(|rest| rest.strip_prefix(": "))
            .unwrap_or(&text);
        Self {
            code,
            message: format!("{}: {what}", path.display()),
        }
    }

    fn not_utf8(path: &Path) -> Self {
        Self {
            code: 2,
            message: format!("{}: a path that is not UTF-8", path.display()),
        }
    }

    /// The configuration did not read; `error` names the file.
    fn of_configuration(error: &StoreError) -> Self {
        Self {
            code: store_code(error),
            message: error.to_string(),
        }
    }

    fn of_document(path: &Path, at: &StorePath, error: &DocumentError) -> Self {
        let code = match error {
            DocumentError::Form { error: _, path: _ } => 1,
            DocumentError::Store(store) => store_code(store),
            _ => 4,
        };
        Self::naming(path, Some(at), code, error)
    }

    fn of_store(path: &Path, at: Option<&StorePath>, error: &StoreError) -> Self {
        Self::naming(path, at, store_code(error), error)
    }

    fn stream(name: &str, error: &io::Error) -> Self {
        Self {
            code: 4,
            message: format!("{name}: {error}"),
        }
    }
}

impl Located<'_> {
    /// Creates the place the store keeps its files when it is missing, such
    /// as a bucket. A store that has none has nothing to create.
    async fn ensure(&self, path: &Path) -> Result<(), Failure> {
        match self.store.ensure_container().await {
            Ok(()) | Err(StoreError::Unsupported(_)) => Ok(()),
            Err(error) => Err(self.name.as_ref().map_or_else(
                || Failure::of_store(path, None, &error),
                |name| name.failure(&error),
            )),
        }
    }
}

impl Named {
    /// `error` of the store this names, on one line that names it.
    fn failure(&self, error: &StoreError) -> Failure {
        Failure {
            code: store_code(error),
            message: self.message(error),
        }
    }

    /// `what` naming this store: in place of the text in backticks where it
    /// shows it, else in front of it.
    fn message(&self, what: &dyn Display) -> String {
        let shown = what.to_string();
        let quoted = format!("`{}`", self.text);
        if shown.contains(&quoted) {
            shown.replace(&quoted, &self.label)
        } else {
            format!("{}: {shown}", self.label)
        }
    }

    /// The store string `text`, the value of the variable `variable`.
    fn variable(variable: &str, text: String) -> Self {
        Self {
            label: variable.to_owned(),
            text,
        }
    }

    /// The store string `text`, written on the command line or saved.
    fn written(text: &str) -> Self {
        Self {
            label: format!("`{text}`"),
            text: text.to_owned(),
        }
    }
}

impl Settings {
    fn new(
        format: FormatArg,
        frontmatter: SyntaxArg,
        allow_missing: bool,
        lock: LockSettings,
    ) -> Self {
        let mut options = WriteOptions::default();
        options.frontmatter = frontmatter.into();
        let missing = if allow_missing {
            Missing::Keep
        } else {
            Missing::Refuse
        };
        Self {
            form: format.into(),
            lock,
            missing,
            options,
        }
    }
}

impl<'system> Setup<'system> {
    /// The store a get or a write of `path` works on, and the path in it: the
    /// store `path` names as a store string; else `path` as written, in the
    /// first store given of `named` (`--store`), `ARKY_STORE` and the
    /// configuration's `default`; else `path` against the current folder, in
    /// the file system.
    async fn locate(&self, named: Option<&StoreArg>, path: &Path) -> Result<Located<'_>, Failure> {
        if let Some(text) = path.to_str() {
            let opened = self
                .backends
                .open_path(text, &self.current, self.configuration.lock, self.system)
                .await
                .map_err(|error| Named::written(text).failure(&error))?;
            if let Some((store, at)) = opened {
                let name = Some(Named::written(text));
                return Ok(Located { at, name, store });
            }
        }
        let chosen = if let Some(StoreArg(text)) = named {
            Some(Named::written(text))
        } else if let Some(text) = variable(
            self.system,
            STORE_VARIABLE,
            "the store get and write use when --store names none",
        )? {
            Some(Named::variable(STORE_VARIABLE, text))
        } else {
            self.configuration
                .default
                .as_ref()
                .map(|default| Named::written(&format!("arky://{default}")))
        };
        let Some(name) = chosen else {
            let at = store_path(self.system, path)?;
            let store = Box::new(machine(self.system, self.configuration.lock));
            return Ok(Located {
                at,
                name: None,
                store,
            });
        };
        let at = as_written(path)?;
        let store = self.open(&name).await?;
        Ok(Located {
            at,
            name: Some(name),
            store,
        })
    }

    /// The configuration and the backends of `system`: the one file `named`
    /// (`--config`) or `ARKY_CONFIG` names, else every `arky.toml` from the
    /// current folder up; the built-in backends and the saved stores.
    fn new(system: &'system dyn System, named: Option<&Path>) -> Result<Self, Failure> {
        let current = system
            .current_dir()
            .map_err(|error| Failure::stream("the current folder", &error))?;
        let file = match named {
            Some(file) => Some(file.to_path_buf()),
            None => variable(system, CONFIG_VARIABLE, "the configuration file")?.map(PathBuf::from),
        };
        let configuration = file
            .map_or_else(
                || Configuration::discover(system, &current),
                |one| Configuration::read(system, &one),
            )
            .map_err(|error| Failure::of_configuration(&error))?;
        let backends = Backends::default().with(SavedBackend::new(configuration.stores.clone()));
        Ok(Self {
            backends,
            configuration,
            current,
            system,
        })
    }

    /// The store `name` names, opened against the current folder; a short form
    /// is refused rather than read as a folder.
    async fn open(&self, name: &Named) -> Result<Box<dyn DynStore + '_>, Failure> {
        if let Some(refusal) = short_form(&name.text) {
            return Err(Failure {
                code: 2,
                message: name.message(&refusal),
            });
        }
        self.backends
            .open(
                &name.text,
                &self.current,
                self.configuration.lock,
                self.system,
            )
            .await
            .map_err(|error| name.failure(&error))
    }
}

/// `path` as the command line wrote it, as a path of a store a store string
/// names.
fn as_written(path: &Path) -> Result<StorePath, Failure> {
    let text = path.to_str().ok_or_else(|| Failure::not_utf8(path))?;
    StorePath::new(text).map_err(|error| Failure::of_store(path, None, &error))
}

/// Runs `cli`'s command over `system`, the configuration read first.
async fn execute(
    cli: &Cli,
    system: &dyn System,
    stdin: &mut (dyn Read + Send),
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    let setup = Setup::new(system, cli.config.as_deref())?;
    let lock = setup.configuration.lock;
    match &cli.command {
        Command::Asset {
            command: AssetCommand::FinishDeletions { store },
        } => finish_deletions(&setup, store, stdout).await,
        Command::Fix { path, store } => fix(&setup, store.as_ref(), path.as_deref(), stdout).await,
        Command::Get {
            path,
            allow_missing,
            embed,
            format,
            frontmatter,
            output,
            store,
        } => {
            let settings = Settings::new(*format, *frontmatter, *allow_missing, lock);
            get(
                &setup,
                store.as_ref(),
                path,
                &settings,
                output.as_deref(),
                *embed,
                stdout,
            )
            .await
        }
        Command::Write {
            path,
            allow_missing,
            format,
            frontmatter,
            input,
            store,
        } => {
            let settings = Settings::new(*format, *frontmatter, *allow_missing, lock);
            write(
                &setup,
                store.as_ref(),
                path,
                &settings,
                input.as_deref(),
                stdin,
            )
            .await
        }
    }
}

/// Removes each expired lock under `path` in the store `named` names, the
/// file system when it is `None`, and writes the path of each one it removed
/// to `stdout`, one per line.
async fn fix(
    setup: &Setup<'_>,
    named: Option<&StoreArg>,
    path: Option<&Path>,
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    let removed = match named {
        None => fix_files(setup.system, path, setup.configuration.lock).await?,
        Some(StoreArg(text)) => {
            let name = Named::written(text);
            let folder = path.map(as_written).transpose()?;
            let store = setup.open(&name).await?;
            match store.remove_expired_locks(folder.as_ref()).await {
                Ok(removed) => removed.iter().map(ToString::to_string).collect(),
                Err(StoreError::Unsupported(_)) => return Err(Failure::keeps_no_locks(&name)),
                Err(error) => return Err(name.failure(&error)),
            }
        }
    };
    for line in removed {
        writeln!(stdout, "{line}").map_err(|error| Failure::stream("standard output", &error))?;
    }
    stdout
        .flush()
        .map_err(|error| Failure::stream("standard output", &error))
}

/// `fix` over the file system: the lock paths below the folder `path` (the
/// current folder when `None`), after `path` as the command line gave it.
async fn fix_files(
    system: &dyn System,
    path: Option<&Path>,
    lock: LockSettings,
) -> Result<Vec<String>, Failure> {
    let given = path.unwrap_or_else(|| Path::new("."));
    let is_folder = system
        .is_dir(&system_path(system, given)?)
        .map_err(|error| Failure::naming(given, None, 4, &error))?;
    if !is_folder {
        return Err(Failure::naming(given, None, 2, &"not a folder"));
    }
    let under = folder_path(system, given)?;
    let removed = machine(system, lock)
        .remove_expired_locks(under.as_ref())
        .await
        .map_err(|error| Failure::of_store(given, under.as_ref(), &error))?;
    Ok(removed
        .iter()
        .map(|found| {
            let below = under
                .as_ref()
                .and_then(|folder| found.as_str().strip_prefix(folder.as_str()))
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(found.as_str());
            path.map_or_else(
                || below.to_owned(),
                |folder| folder.join(below).display().to_string(),
            )
        })
        .collect())
}

/// Finishes each deletion of an embedded file that stopped halfway in the
/// store `named` names, and writes the store path of each object it deleted
/// to `stdout`, one per line.
async fn finish_deletions(
    setup: &Setup<'_>,
    named: &StoreArg,
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    let name = Named::written(&named.0);
    let store = setup.open(&name).await?;
    let finished = match store.finish_deletions().await {
        Ok(finished) => finished,
        Err(StoreError::Unsupported(_)) => return Err(Failure::keeps_no_deletions(&name)),
        Err(error) => return Err(name.failure(&error)),
    };
    for object in finished {
        writeln!(stdout, "{object}").map_err(|error| Failure::stream("standard output", &error))?;
    }
    stdout
        .flush()
        .map_err(|error| Failure::stream("standard output", &error))
}

/// The store path of the folder `path`: `None` for the root of the file
/// system.
fn folder_path(system: &dyn System, path: &Path) -> Result<Option<StorePath>, Failure> {
    let text = store_text(system, path)?;
    if text.is_empty() {
        return Ok(None);
    }
    StorePath::new(&text)
        .map(Some)
        .map_err(|error| Failure::of_store(path, None, &error))
}

/// Writes the document at `path` to `stdout`, or to the file `output`, whole.
/// Markdown to `output` is that file and each file it embeds beside it, unless
/// `embed` asks for the one file.
async fn get(
    setup: &Setup<'_>,
    named: Option<&StoreArg>,
    path: &Path,
    settings: &Settings,
    output: Option<&Path>,
    embed: bool,
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    if embed && settings.form != Form::Markdown {
        return Err(Failure {
            code: 2,
            message: "--embed writes markdown; it needs -f markdown".to_owned(),
        });
    }
    let system = setup.system;
    let located = setup.locate(named, path).await?;
    let target = output
        .map(|file| store_path(system, file).map(|place| (file, place)))
        .transpose()?;
    let bundle = document::get(located.store.as_ref(), &located.at, settings.missing)
        .await
        .map_err(|error| Failure::of_document(path, &located.at, &error))?;
    if let Some((file, place)) = &target
        && settings.form == Form::Markdown
        && !embed
    {
        let _: Revision = document::put_bundle(
            &machine(system, settings.lock),
            place,
            &bundle,
            &settings.options,
            &Precondition::None,
        )
        .await
        .map_err(|error| Failure::of_document(file, place, &error))?;
        return Ok(());
    }
    let bytes = encode(&bundle, settings.form, &settings.options)
        .map_err(|error| Failure::naming(path, None, 1, &error))?;
    if let Some((file, place)) = target {
        let _: Revision = machine(system, settings.lock)
            .write(&place, &bytes, &Precondition::None)
            .await
            .map_err(|error| Failure::of_store(file, Some(&place), &error))?;
        return Ok(());
    }
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| Failure::stream("standard output", &error))
}

/// The files of the machine, as a store whose root is `/`.
fn machine(system: &dyn System, lock: LockSettings) -> FileStore<'_> {
    FileStore::new(PathBuf::from("/"), system).locking(lock)
}

/// Writes `failure` to `stderr` as one line, and gives its exit code.
fn report(stderr: &mut (dyn Write + Send), failure: &Failure) -> ExitCode {
    let _: io::Result<()> = writeln!(stderr, "arky: {}", failure.message);
    ExitCode::from(failure.code)
}

/// Runs `cli` over `system` and the three streams, and gives the exit code.
pub async fn run(
    cli: &Cli,
    system: &dyn System,
    stdin: &mut (dyn Read + Send),
    stdout: &mut (dyn Write + Send),
    stderr: &mut (dyn Write + Send),
) -> ExitCode {
    execute(cli, system, stdin, stdout)
        .await
        .map_or_else(|failure| report(stderr, &failure), |()| ExitCode::SUCCESS)
}

/// The refusal of `text` when it is one of the short forms, such as
/// `minio:notes`: a folder of that name is written `./minio:notes`.
fn short_form(text: &str) -> Option<String> {
    let (scheme, rest) = text.split_once(':')?;
    if rest.starts_with("//") {
        return None;
    }
    SHORT_FORMS
        .iter()
        .find(|(short, _, _)| *short == scheme)
        .map(|(_, store, form)| {
            format!(
                "{scheme}: is not a store string: {store} is {form}, and a folder of this name starts with ./"
            )
        })
}

/// The exit code of a store's error.
const fn store_code(error: &StoreError) -> u8 {
    match error {
        StoreError::InvalidPath(_) | StoreError::InvalidStore(_) => 2,
        StoreError::NotConfigured(_) => 3,
        StoreError::NotFound(_) => 6,
        StoreError::Backend(_) | StoreError::Conflict(_) | StoreError::Unsupported(_) | _ => 4,
    }
}

/// `path` against the current folder, `.` and `..` resolved from its text
/// without following links, as a path of the store whose root is `/`.
fn store_path(system: &dyn System, path: &Path) -> Result<StorePath, Failure> {
    StorePath::new(&store_text(system, path)?)
        .map_err(|error| Failure::of_store(path, None, &error))
}

/// The text of the store path `path` names, empty for the root.
fn store_text(system: &dyn System, path: &Path) -> Result<String, Failure> {
    let full = system
        .current_dir()
        .map_err(|error| Failure::stream("the current folder", &error))?
        .join(path);
    let mut parts = Vec::new();
    for component in full.components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or_else(|| Failure::not_utf8(path))?);
            }
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    Ok(parts.join("/"))
}

/// `file` against the current folder.
fn system_path(system: &dyn System, file: &Path) -> Result<PathBuf, Failure> {
    system
        .current_dir()
        .map(|folder| folder.join(file))
        .map_err(|error| Failure::stream("the current folder", &error))
}

/// The value of the variable `name`, which holds `holds`, or `None` when it
/// is not set; the error names the variable, never its value.
fn variable(system: &dyn System, name: &str, holds: &str) -> Result<Option<String>, Failure> {
    match system.env_var(name) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(Failure::configuration(format!(
            "{name} is not UTF-8: it holds {holds}"
        ))),
    }
}

/// Stores at `path` the document read from `stdin`, or from the file `input`,
/// replacing any file there. Markdown from `input` brings each file it embeds
/// from beside it. The store's container is created, when it is missing,
/// after the input is read and before the first write.
async fn write(
    setup: &Setup<'_>,
    named: Option<&StoreArg>,
    path: &Path,
    settings: &Settings,
    input: Option<&Path>,
    stdin: &mut (dyn Read + Send),
) -> Result<(), Failure> {
    let system = setup.system;
    let located = setup.locate(named, path).await?;
    let stored = if let Some(file) = input
        && settings.form == Form::Markdown
    {
        let source = store_path(system, file)?;
        let bundle = document::get(&machine(system, settings.lock), &source, settings.missing)
            .await
            .map_err(|error| Failure::of_document(file, &source, &error))?;
        located.ensure(path).await?;
        document::put_bundle(
            located.store.as_ref(),
            &located.at,
            &bundle,
            &settings.options,
            &Precondition::None,
        )
        .await
    } else {
        let bytes = if let Some(file) = input {
            let source = store_path(system, file)?;
            machine(system, settings.lock)
                .read(&source)
                .await
                .map_err(|error| Failure::of_store(file, Some(&source), &error))?
                .bytes
        } else {
            let mut bytes = Vec::new();
            stdin
                .read_to_end(&mut bytes)
                .map_err(|error| Failure::stream("standard input", &error))?;
            bytes
        };
        located.ensure(path).await?;
        document::put(
            located.store.as_ref(),
            &located.at,
            settings.form,
            &bytes,
            &settings.options,
            &Precondition::None,
            settings.missing,
        )
        .await
    };
    let _: Revision = stored.map_err(|error| Failure::of_document(path, &located.at, &error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
