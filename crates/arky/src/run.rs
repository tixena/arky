//! The commands, run over a system and the three standard streams.

use core::fmt::Display;
#[cfg(not(feature = "mongodb"))]
use core::future::{self, Ready};
use core::time::Duration;
use std::env::VarError;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use arky_core::bundle::Missing;
use arky_core::forms::{Form, encode};
use arky_core::parser::ast::{Mapping, Value};
use arky_core::parser::read_toml;
use arky_core::writer::WriteOptions;
use arky_store::any::AnyStore;
use arky_store::document::{self, DocumentError};
use arky_store::file::FileStore;
use arky_store::lock::LockSettings;
#[cfg(feature = "mongodb")]
use arky_store::mongo::{MongoStore, object_path};
#[cfg(any(feature = "minio", feature = "mongodb"))]
use arky_store::object::{ObjectConfig, ObjectStore};
use arky_store::{Precondition, Revision, Store as _, StoreError, StorePath};
use os_shim::System;
#[cfg(any(feature = "minio", feature = "mongodb"))]
use os_shim::bucket::MinioBucket;

#[cfg(feature = "mongodb")]
use crate::cli::bucket_and_prefix;
use crate::cli::{AssetCommand, Cli, Command, FormatArg, StoreArg, SyntaxArg};

/// The configuration file arky reads in the current folder when none is
/// named.
const CONFIG_FILE: &str = "arky.toml";

/// Why a command stopped: the line after `arky: `, and the exit code.
struct Failure {
    code: u8,
    message: String,
}

/// What `--format`, `--frontmatter` and `--allow-missing` ask for: the form,
/// the options the document is written with, and what an embed with no file
/// does; and the lock the configuration file sets.
struct Settings {
    form: Form,
    lock: LockSettings,
    missing: Missing,
    options: WriteOptions,
}

impl Failure {
    const fn configuration(message: String) -> Self {
        Self { code: 3, message }
    }

    /// `what` went wrong with the configuration file `file`.
    fn in_file(file: &Path, what: &dyn Display) -> Self {
        Self::configuration(format!("{}: {what}", file.display()))
    }

    /// `asset finish-deletions` given `arg`, a store that keeps no deletions.
    fn keeps_no_deletions(arg: &StoreArg) -> Self {
        Self {
            code: 2,
            message: format!(
                "{arg} keeps no deletions: asset finish-deletions takes --store mongodb:<database>/<collection>"
            ),
        }
    }

    /// `fix` given `arg`, a store that keeps no locks.
    fn keeps_no_locks(arg: &StoreArg) -> Self {
        Self {
            code: 2,
            message: format!(
                "{arg} keeps no locks: fix takes the file system or --store minio:<bucket>"
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

    /// The store `arg` names failed to open.
    #[cfg(any(feature = "minio", feature = "mongodb"))]
    fn store(arg: &StoreArg, error: &StoreError) -> Self {
        Self {
            code: store_code(error),
            message: format!("{arg}: {error}"),
        }
    }

    fn stream(name: &str, error: &io::Error) -> Self {
        Self {
            code: 4,
            message: format!("{name}: {error}"),
        }
    }

    #[cfg(not(all(feature = "minio", feature = "mongodb")))]
    fn unbuilt(arg: &StoreArg, store: &str) -> Self {
        Self {
            code: 3,
            message: format!("{arg}: this arky is built without the {store} store"),
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

/// `path` as the command line wrote it, as a path of a store `--store` names.
#[cfg(any(feature = "minio", feature = "mongodb"))]
fn as_written(path: &Path) -> Result<StorePath, Failure> {
    let text = path.to_str().ok_or_else(|| Failure::not_utf8(path))?;
    StorePath::new(text).map_err(|error| Failure::of_store(path, None, &error))
}

/// Removes each expired lock under `path` in the store `named` names, and
/// writes the path of each one it removed to `stdout`, one per line.
async fn fix(
    system: &dyn System,
    named: Option<&StoreArg>,
    path: Option<&Path>,
    lock: LockSettings,
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    let removed = match named {
        None => fix_files(system, path, lock).await?,
        Some(arg) => match arg {
            #[cfg(feature = "minio")]
            StoreArg::Minio { bucket, prefix } => {
                fix_objects(system, arg, bucket, prefix.as_deref(), path, lock).await?
            }
            #[cfg(not(feature = "minio"))]
            StoreArg::Minio {
                bucket: _,
                prefix: _,
            } => return Err(Failure::unbuilt(arg, "MinIO")),
            StoreArg::Mongodb {
                collection: _,
                database: _,
            } => return Err(Failure::keeps_no_locks(arg)),
        },
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

/// `fix` over the `MinIO` store `arg` names: the store paths of the locks
/// under `path`, the whole prefix when `None`.
#[cfg(feature = "minio")]
async fn fix_objects(
    system: &dyn System,
    arg: &StoreArg,
    bucket: &str,
    prefix: Option<&str>,
    path: Option<&Path>,
    lock: LockSettings,
) -> Result<Vec<String>, Failure> {
    let under = prefix
        .map(StorePath::new)
        .transpose()
        .map_err(|error| Failure::store(arg, &error))?;
    let store = objects(system, bucket, under, lock)?;
    let folder = path.map(as_written).transpose()?;
    let removed = store
        .remove_expired_locks(folder.as_ref())
        .await
        .map_err(|error| Failure::store(arg, &error))?;
    Ok(removed.iter().map(ToString::to_string).collect())
}

/// Finishes each deletion of an embedded file that stopped halfway in the
/// MongoDB store `arg` names, and writes the key of each object it deleted to
/// `stdout`, one per line.
#[cfg(feature = "mongodb")]
async fn finish_deletions(
    system: &dyn System,
    arg: &StoreArg,
    stdout: &mut (dyn Write + Send),
) -> Result<(), Failure> {
    match arg {
        StoreArg::Minio {
            bucket: _,
            prefix: _,
        } => Err(Failure::keeps_no_deletions(arg)),
        StoreArg::Mongodb {
            collection,
            database,
        } => {
            let store = mongodb(system, arg, database, collection).await?;
            let finished = store
                .finish_deletions()
                .await
                .map_err(|error| Failure::store(arg, &error))?;
            for hash in finished {
                let object = object_path(&hash).map_err(|error| Failure::store(arg, &error))?;
                writeln!(stdout, "{object}")
                    .map_err(|error| Failure::stream("standard output", &error))?;
            }
            stdout
                .flush()
                .map_err(|error| Failure::stream("standard output", &error))
        }
    }
}

/// The failure of `asset finish-deletions` with `arg` in a build without the
/// MongoDB store, the one store that keeps deletions.
#[cfg(not(feature = "mongodb"))]
fn finish_deletions(arg: &StoreArg) -> Result<(), Failure> {
    Err(match arg {
        StoreArg::Minio {
            bucket: _,
            prefix: _,
        } => Failure::keeps_no_deletions(arg),
        StoreArg::Mongodb {
            collection: _,
            database: _,
        } => Failure::unbuilt(arg, "MongoDB"),
    })
}

/// Writes the document at `path` to `stdout`, or to the file `output`, whole.
/// Markdown to `output` is that file and each file it embeds beside it, unless
/// `embed` asks for the one file.
async fn get(
    system: &dyn System,
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
    let (store, at) = open(system, named, path, settings.lock).await?;
    let target = output
        .map(|file| store_path(system, file).map(|place| (file, place)))
        .transpose()?;
    let bundle = document::get(&store, &at, settings.missing)
        .await
        .map_err(|error| Failure::of_document(path, &at, &error))?;
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

/// The `MinIO` store `arg` names, `bucket` under `prefix` on the server the
/// environment names, and `path` as a path of it.
#[cfg(feature = "minio")]
fn minio<'system>(
    system: &'system dyn System,
    arg: &StoreArg,
    bucket: &str,
    prefix: Option<&str>,
    path: &Path,
    lock: LockSettings,
) -> Result<(AnyStore<'system>, StorePath), Failure> {
    let under = prefix
        .map(StorePath::new)
        .transpose()
        .map_err(|error| Failure::store(arg, &error))?;
    let store = objects(system, bucket, under, lock)?;
    let at = as_written(path)?;
    Ok((AnyStore::Minio(store), at))
}

/// The MongoDB store `arg` names: the collection `collection` of the database
/// `database`, on the server `ARKY_MONGODB_URI` names, its files in the
/// bucket `ARKY_MONGODB_BUCKET` names.
#[cfg(feature = "mongodb")]
async fn mongodb<'system>(
    system: &'system dyn System,
    arg: &StoreArg,
    database: &str,
    collection: &str,
) -> Result<MongoStore<'system>, Failure> {
    let uri = variable(system, "ARKY_MONGODB_URI", "the MongoDB connection string")?;
    let files = mongodb_files(system)?;
    MongoStore::open(&uri, database, collection, files)
        .await
        .map_err(|error| Failure::store(arg, &error))
}

/// The bucket a MongoDB store keeps its files in: `ARKY_MONGODB_BUCKET`,
/// `<bucket>` or `<bucket>/<prefix>`, on the `MinIO` server the environment
/// names.
#[cfg(feature = "mongodb")]
fn mongodb_files(system: &dyn System) -> Result<ObjectStore<'_, MinioBucket>, Failure> {
    let (name, holds) = (
        "ARKY_MONGODB_BUCKET",
        "the MinIO bucket of a MongoDB store's files",
    );
    let place = variable(system, name, holds)?;
    let (bucket, prefix) = bucket_and_prefix(&place).ok_or_else(|| {
        Failure::configuration(format!(
            "{name} is not <bucket> or <bucket>/<prefix>: it holds {holds}"
        ))
    })?;
    let under = prefix
        .map(StorePath::new)
        .transpose()
        .map_err(|error| Failure::configuration(format!("{name}: {error}")))?;
    objects(system, bucket, under, LockSettings::default())
}

/// The bucket `bucket`, its keys under `prefix`, on the `MinIO` server the
/// environment names.
#[cfg(any(feature = "minio", feature = "mongodb"))]
fn objects<'system>(
    system: &'system dyn System,
    bucket: &str,
    prefix: Option<StorePath>,
    lock: LockSettings,
) -> Result<ObjectStore<'system, MinioBucket>, Failure> {
    let endpoint = variable(system, "ARKY_MINIO_ENDPOINT", "the MinIO server's address")?;
    let region = variable(
        system,
        "ARKY_MINIO_REGION",
        "the region the MinIO server expects",
    )?;
    let config = ObjectConfig::new(endpoint, region, bucket.to_owned(), prefix);
    // `open` fails only on a key missing from the environment.
    ObjectStore::open(system, config)
        .map(|store| store.locking(lock))
        .map_err(|error| Failure::configuration(error.to_string()))
}

/// The store `named` names, the file system's when it is `None`, and `path`
/// as a path of it.
#[cfg(feature = "mongodb")]
async fn open<'system>(
    system: &'system dyn System,
    named: Option<&StoreArg>,
    path: &Path,
    lock: LockSettings,
) -> Result<(AnyStore<'system>, StorePath), Failure> {
    let Some(arg) = named else {
        return Ok((
            AnyStore::File(machine(system, lock)),
            store_path(system, path)?,
        ));
    };
    match arg {
        #[cfg(feature = "minio")]
        StoreArg::Minio { bucket, prefix } => {
            minio(system, arg, bucket, prefix.as_deref(), path, lock)
        }
        #[cfg(not(feature = "minio"))]
        StoreArg::Minio {
            bucket: _,
            prefix: _,
        } => Err(Failure::unbuilt(arg, "MinIO")),
        StoreArg::Mongodb {
            collection,
            database,
        } => {
            let at = as_written(path)?;
            let store = mongodb(system, arg, database, collection).await?;
            Ok((AnyStore::Mongo(store), at))
        }
    }
}

/// The store `named` names, the file system's when it is `None`, and `path`
/// as a path of it: ready at once, as no store of this build connects.
#[cfg(not(feature = "mongodb"))]
fn open<'system>(
    system: &'system dyn System,
    named: Option<&StoreArg>,
    path: &Path,
    lock: LockSettings,
) -> Ready<Result<(AnyStore<'system>, StorePath), Failure>> {
    let Some(arg) = named else {
        let at = store_path(system, path);
        return future::ready(at.map(|place| (AnyStore::File(machine(system, lock)), place)));
    };
    let opened = match arg {
        #[cfg(feature = "minio")]
        StoreArg::Minio { bucket, prefix } => {
            minio(system, arg, bucket, prefix.as_deref(), path, lock)
        }
        #[cfg(not(feature = "minio"))]
        StoreArg::Minio {
            bucket: _,
            prefix: _,
        } => Err(Failure::unbuilt(arg, "MinIO")),
        StoreArg::Mongodb {
            collection: _,
            database: _,
        } => Err(Failure::unbuilt(arg, "MongoDB")),
    };
    future::ready(opened)
}

/// Runs `cli` over `system` and the three streams, and gives the exit code.
pub async fn run(
    cli: &Cli,
    system: &dyn System,
    stdin: &mut (dyn Read + Send),
    stdout: &mut (dyn Write + Send),
    stderr: &mut (dyn Write + Send),
) -> ExitCode {
    let lock = match lock_settings(system, cli.config.as_deref()) {
        Ok(lock) => lock,
        Err(failure) => return report(stderr, &failure),
    };
    let done = match &cli.command {
        #[cfg(feature = "mongodb")]
        Command::Asset {
            command: AssetCommand::FinishDeletions { store },
        } => finish_deletions(system, store, stdout).await,
        #[cfg(not(feature = "mongodb"))]
        Command::Asset {
            command: AssetCommand::FinishDeletions { store },
        } => finish_deletions(store),
        Command::Fix { path, store } => {
            fix(system, store.as_ref(), path.as_deref(), lock, stdout).await
        }
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
                system,
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
                system,
                store.as_ref(),
                path,
                &settings,
                input.as_deref(),
                stdin,
            )
            .await
        }
    };
    done.map_or_else(|failure| report(stderr, &failure), |()| ExitCode::SUCCESS)
}

/// Writes `failure` to `stderr` as one line, and gives its exit code.
fn report(stderr: &mut (dyn Write + Send), failure: &Failure) -> ExitCode {
    let _: io::Result<()> = writeln!(stderr, "arky: {}", failure.message);
    ExitCode::from(failure.code)
}

/// The lock's settings in the `[lock]` table of the configuration file:
/// `named` (from `--config`), else the file `ARKY_CONFIG` names, else
/// `arky.toml` in the current folder; the defaults where that last file is
/// missing or has no `[lock]`.
fn lock_settings(system: &dyn System, named: Option<&Path>) -> Result<LockSettings, Failure> {
    let from_variable = match (named, system.env_var("ARKY_CONFIG")) {
        (Some(_), _) | (None, Err(VarError::NotPresent)) => None,
        (None, Ok(file)) => Some(PathBuf::from(file)),
        (None, Err(VarError::NotUnicode(_))) => {
            return Err(Failure::configuration(
                "ARKY_CONFIG is not UTF-8: it holds the configuration file".to_owned(),
            ));
        }
    };
    let asked = named.map(Path::to_path_buf).or(from_variable);
    let file = asked.clone().unwrap_or_else(|| PathBuf::from(CONFIG_FILE));
    let source = match system.read_to_string(&system_path(system, &file)?) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return if asked.is_some() {
                Err(Failure::in_file(
                    &file,
                    &"the configuration file does not exist",
                ))
            } else {
                Ok(LockSettings::default())
            };
        }
        Err(error) => return Err(Failure::in_file(&file, &error)),
    };
    let tables =
        read_toml(&source, 0..source.len()).map_err(|error| Failure::in_file(&file, &error))?;
    match tables.get("lock") {
        None => Ok(LockSettings::default()),
        Some(Value::Mapping(lock)) => lock_table(&file, lock),
        Some(_) => Err(Failure::in_file(&file, &"lock is not a table")),
    }
}

/// The settings the `[lock]` table `lock` of the configuration file `file`
/// gives: the defaults for a key left out.
fn lock_table(file: &Path, lock: &Mapping) -> Result<LockSettings, Failure> {
    let mut settings = LockSettings::default();
    for (key, value) in lock {
        let seconds = match (key.as_str(), value) {
            ("expires_seconds" | "wait_seconds", Value::Integer(whole)) if *whole > 0 => {
                Duration::from_secs(whole.unsigned_abs())
            }
            ("expires_seconds" | "wait_seconds", _) => {
                return Err(Failure::in_file(
                    file,
                    &format!("{key} in [lock] is not a whole number of seconds above zero"),
                ));
            }
            _ => {
                return Err(Failure::in_file(
                    file,
                    &format!("[lock] has no key {key}; it takes expires_seconds and wait_seconds"),
                ));
            }
        };
        if key == "expires_seconds" {
            settings.expires = seconds;
        } else {
            settings.wait = seconds;
        }
    }
    Ok(settings)
}

/// `file` against the current folder.
fn system_path(system: &dyn System, file: &Path) -> Result<PathBuf, Failure> {
    system
        .current_dir()
        .map(|folder| folder.join(file))
        .map_err(|error| Failure::stream("the current folder", &error))
}

/// The exit code of a store's error.
const fn store_code(error: &StoreError) -> u8 {
    match error {
        StoreError::InvalidPath(_) => 2,
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

/// The value of the variable `name`, which holds `holds`; the error names the
/// variable, never its value.
#[cfg(any(feature = "minio", feature = "mongodb"))]
fn variable(system: &dyn System, name: &str, holds: &str) -> Result<String, Failure> {
    system.env_var(name).map_err(|error| {
        let what = match error {
            VarError::NotPresent => "is not set",
            VarError::NotUnicode(_) => "is not UTF-8",
        };
        Failure::configuration(format!("{name} {what}: it holds {holds}"))
    })
}

/// Stores at `path` the document read from `stdin`, or from the file `input`,
/// replacing any file there. Markdown from `input` brings each file it embeds
/// from beside it.
async fn write(
    system: &dyn System,
    named: Option<&StoreArg>,
    path: &Path,
    settings: &Settings,
    input: Option<&Path>,
    stdin: &mut (dyn Read + Send),
) -> Result<(), Failure> {
    let (store, at) = open(system, named, path, settings.lock).await?;
    let stored = if let Some(file) = input
        && settings.form == Form::Markdown
    {
        let source = store_path(system, file)?;
        let bundle = document::get(&machine(system, settings.lock), &source, settings.missing)
            .await
            .map_err(|error| Failure::of_document(file, &source, &error))?;
        document::put_bundle(&store, &at, &bundle, &settings.options, &Precondition::None).await
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
        document::put(
            &store,
            &at,
            settings.form,
            &bytes,
            &settings.options,
            &Precondition::None,
            settings.missing,
        )
        .await
    };
    let _: Revision = stored.map_err(|error| Failure::of_document(path, &at, &error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
