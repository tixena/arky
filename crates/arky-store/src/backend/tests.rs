//! Store strings over `MemorySystem`: the expansion of their variables, the
//! file, `MinIO` and AWS S3 backends, a backend of a test's own, a store
//! string in a path argument, and a folder inside a store; under
//! `cfg(store_tests)`, the `MinIO` backend on the `RustFS` server
//! `just test-stores` starts.

#[cfg(all(store_tests, feature = "minio"))]
mod server;

use alloc::collections::BTreeMap;
#[cfg(feature = "s3")]
use alloc::sync::Arc;
use core::future;
use core::time::Duration;
use std::path::{Path, PathBuf};
#[cfg(feature = "s3")]
use std::sync::Mutex;

use os_shim::System as _;
#[cfg(feature = "s3")]
use os_shim::bucket::{Bucket, Condition, Head, MemoryBucket, Object, ObjectError};
use os_shim::mock::MemorySystem;

#[cfg(any(feature = "minio", feature = "s3"))]
use super::Keys;
use super::expand::expand;
use super::file::FileBackend;
#[cfg(feature = "minio")]
use super::minio::{self, Minio, MinioBackend};
#[cfg(feature = "s3")]
use super::s3::{self, S3, S3Backend};
use super::{Backend, Backends, Context, within};
#[cfg(feature = "s3")]
use crate::Store;
use crate::file::FileStore;
use crate::lock::LockSettings;
#[cfg(feature = "s3")]
use crate::object::{EnsureBucket, ObjectStore};
use crate::{BoxFuture, Capabilities, DynStore, Precondition, StoreError, StorePath};

/// The folder a relative path is relative to in these tests.
const BASE: &str = "/v";

/// The `MinIO` store string of the expansion table.
const MINIO_NOTES: &str = "minio+https://${MINIO_USER}:${MINIO_PASSWORD}@minio.example.com/notes";

/// `MINIO_PASSWORD`, which no error message may show.
const PASSWORD: &str = "p@ss/word";

/// `MINIO_PASSWORD` percent-encoded, as a store string holds it.
const PASSWORD_ENCODED: &str = "p%40ss%2Fword";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

/// A backend of a test's own: `memory://<name>` is a file store at
/// `/memory/<name>`.
struct MemoryBackend;

impl Backend for MemoryBackend {
    fn open<'call, 'store>(
        &'store self,
        text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        let name = text.trim_start_matches("memory://");
        if name.is_empty() {
            return Box::pin(future::ready(Err(StoreError::InvalidStore(
                "no name".to_owned(),
            ))));
        }
        let root = PathBuf::from("/memory").join(name);
        let store: Box<dyn DynStore + 'store> =
            Box::new(FileStore::new(root, context.system).locking(context.lock));
        Box::pin(future::ready(Ok(store)))
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["memory"]
    }

    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        let rest = text.trim_start_matches("memory://");
        let (name, inside) = rest.split_once('/').unwrap_or((rest, ""));
        Ok((format!("memory://{name}"), inside.to_owned()))
    }
}

/// A backend that answers `file` with a file store at `/overridden`.
struct OverFile;

impl Backend for OverFile {
    fn open<'call, 'store>(
        &'store self,
        _text: &'call str,
        context: Context<'store>,
    ) -> BoxFuture<'call, Result<Box<dyn DynStore + 'store>, StoreError>>
    where
        'store: 'call,
    {
        let store: Box<dyn DynStore + 'store> =
            Box::new(FileStore::new(PathBuf::from("/overridden"), context.system));
        Box::pin(future::ready(Ok(store)))
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["file"]
    }

    fn split(&self, text: &str) -> Result<(String, String), StoreError> {
        Ok(("file:".to_owned(), text.to_owned()))
    }
}

/// A `MemoryBucket` that records the KMS key of each `ensure_bucket`.
#[cfg(feature = "s3")]
#[derive(Clone, Default)]
struct Recorded {
    bucket: MemoryBucket,
    kms: Arc<Mutex<Vec<Option<String>>>>,
}

#[cfg(feature = "s3")]
impl Bucket for Recorded {
    fn delete(
        &self,
        key: &str,
        condition: &Condition,
    ) -> impl Future<Output = Result<(), ObjectError>> + Send {
        self.bucket.delete(key, condition)
    }

    fn get(&self, key: &str) -> impl Future<Output = Result<Object, ObjectError>> + Send {
        self.bucket.get(key)
    }

    fn head(&self, key: &str) -> impl Future<Output = Result<Head, ObjectError>> + Send {
        self.bucket.head(key)
    }

    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>, ObjectError>> + Send {
        self.bucket.list(prefix)
    }

    fn put(
        &self,
        key: &str,
        bytes: &[u8],
        condition: &Condition,
    ) -> impl Future<Output = Result<String, ObjectError>> + Send {
        self.bucket.put(key, bytes, condition)
    }
}

#[cfg(feature = "s3")]
impl EnsureBucket for Recorded {
    fn ensure_bucket(
        &self,
        kms_key_arn: Option<&str>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        self.kms
            .lock()
            .unwrap()
            .push(kms_key_arn.map(ToOwned::to_owned));
        future::ready(Ok(()))
    }
}

/// The file store at `/notes` over `system`, seen through `within(…,
/// drafts)`.
fn drafts(system: &MemorySystem) -> Box<dyn DynStore + '_> {
    let store = Box::new(FileStore::new(PathBuf::from("/notes"), system));
    within(store, path("drafts"))
}

fn path(text: &str) -> StorePath {
    StorePath::new(text).unwrap()
}

/// A system whose environment holds `HOME`, `MINIO_USER` and
/// `MINIO_PASSWORD`.
fn secrets() -> MemorySystem {
    MemorySystem::new()
        .with_env("HOME", "/home/me")
        .unwrap()
        .with_env("MINIO_PASSWORD", PASSWORD)
        .unwrap()
        .with_env("MINIO_USER", "AKIA0000")
        .unwrap()
}

/// A system whose environment holds the two AWS variables.
#[cfg(any(feature = "minio", feature = "s3"))]
fn with_aws_keys() -> MemorySystem {
    MemorySystem::new()
        .with_env("AWS_ACCESS_KEY_ID", "AKIA0000")
        .unwrap()
        .with_env("AWS_SECRET_ACCESS_KEY", "a-secret")
        .unwrap()
}

#[test]
fn a_variable_in_a_store_string_is_percent_encoded() {
    let expanded = expand(MINIO_NOTES, &secrets(), true)
        .unwrap()
        .text()
        .to_owned();
    assert_eq!(
        expanded,
        "minio+https://AKIA0000:p%40ss%2Fword@minio.example.com/notes"
    );
}

#[test]
fn a_variable_in_a_plain_path_is_taken_as_it_is() {
    let expanded = expand("${HOME}/notes", &secrets(), false)
        .unwrap()
        .text()
        .to_owned();
    assert_eq!(expanded, "/home/me/notes");
}

#[test]
fn two_dollars_write_one_and_a_lone_dollar_stays() {
    let system = secrets();
    assert_eq!(
        expand("s3://cost$$center", &system, true).unwrap().text(),
        "s3://cost$center"
    );
    assert_eq!(
        expand("s3://cost$center", &system, true).unwrap().text(),
        "s3://cost$center"
    );
}

#[test]
fn a_variable_not_set_is_not_configured_naming_it_and_the_string_as_written() {
    let system = MemorySystem::new()
        .with_env("MINIO_USER", "AKIA0000")
        .unwrap();
    let refused = StoreError::NotConfigured(format!(
        "MINIO_PASSWORD is not set: `{MINIO_NOTES}` reads it"
    ));
    assert_eq!(expand(MINIO_NOTES, &system, true).err(), Some(refused));
}

#[test]
fn a_dollar_brace_with_no_closing_brace_or_no_name_is_an_invalid_store() {
    let system = secrets();
    let unclosed = StoreError::InvalidStore("`${HOME`: a ${ with no closing }".to_owned());
    assert_eq!(expand("${HOME", &system, false).err(), Some(unclosed));
    let nameless = StoreError::InvalidStore("`file:${}`: a ${} with no name".to_owned());
    assert_eq!(expand("file:${}", &system, true).err(), Some(nameless));
}

#[tokio::test]
async fn no_error_message_shows_a_variable_s_value() {
    let system = secrets();
    let backends = Backends::default();
    let base = Path::new(BASE);
    let lock = LockSettings::default();
    let refused = [
        "file:${MINIO_PASSWORD}?${UNSET}",
        "file:${MINIO_PASSWORD}?x=${MINIO_PASSWORD}",
        "file:${MINIO_PASSWORD}/${HOME",
        "hdfs://${MINIO_USER}:${MINIO_PASSWORD}@namenode/vault",
        #[cfg(feature = "minio")]
        "minio+http://${MINIO_USER}:${MINIO_PASSWORD}@127.0.0.1:9000/notes?regoin=${MINIO_PASSWORD}",
        #[cfg(feature = "minio")]
        "minio+http://${MINIO_USER}:${MINIO_PASSWORD}@127.0.0.1:9000/",
        #[cfg(feature = "minio")]
        "minio+http://${MINIO_USER}:${MINIO_PASSWORD}@127.0.0.1:99999/notes",
        #[cfg(feature = "minio")]
        "minio+http://${MINIO_USER}:${MINIO_PASSWORD}@127.0.0.1:9000/notes#${MINIO_PASSWORD}",
        #[cfg(feature = "minio")]
        "minio+http://${MINIO_PASSWORD}@127.0.0.1:9000/notes",
        #[cfg(feature = "s3")]
        "s3://${MINIO_USER}:${MINIO_PASSWORD}@acme?regoin=eu-west-1",
        #[cfg(feature = "s3")]
        "s3://${MINIO_USER}:${MINIO_PASSWORD}@acme",
        #[cfg(feature = "s3")]
        "s3://${MINIO_USER}:${MINIO_PASSWORD}@acme:9000?region=eu-west-1",
    ];
    for text in refused {
        let message = backends
            .open(text, base, lock, &system)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains(text), "{message}");
        assert!(!message.contains(PASSWORD), "{message}");
        assert!(!message.contains(PASSWORD_ENCODED), "{message}");
    }
    let in_paths = [
        "file:x/${MINIO_PASSWORD}/../a.md",
        "file:${MINIO_PASSWORD}/a.md?${MINIO_PASSWORD}",
        #[cfg(feature = "s3")]
        "s3://acme/${MINIO_PASSWORD}/a.md",
    ];
    for text in in_paths {
        let message = backends
            .open_path(text, base, lock, &system)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains(text), "{message}");
        assert!(!message.contains(PASSWORD), "{message}");
        assert!(!message.contains(PASSWORD_ENCODED), "{message}");
    }
}

#[tokio::test]
async fn a_file_string_or_a_plain_path_names_its_root_against_the_base_folder() {
    let roots = [
        ("file:///srv/notes", "/srv/notes"),
        ("file:/srv/notes", "/srv/notes"),
        ("file:notes", "/v/notes"),
        ("file:./notes", "/v/notes"),
        ("file:.", "/v"),
        ("file:../w", "/w"),
        ("file:my%20notes", "/v/my notes"),
        ("./notes", "/v/notes"),
        ("notes", "/v/notes"),
        ("/srv/notes", "/srv/notes"),
        ("my%20notes", "/v/my%20notes"),
        ("notes:report.md", "/v/notes:report.md"),
    ];
    for (text, root) in roots {
        let system = MemorySystem::new();
        let backends = Backends::default();
        let store = backends
            .open(text, Path::new(BASE), LockSettings::default(), &system)
            .await
            .unwrap();
        store
            .write(&path("a.md"), b"# A\n", &Precondition::Absent)
            .await
            .unwrap();
        let written = system.read_to_string(&Path::new(root).join("a.md"));
        assert_eq!(written.unwrap(), "# A\n", "{text}");
    }
}

#[tokio::test]
async fn a_file_string_with_an_option_is_an_invalid_store_naming_it() {
    let system = MemorySystem::new();
    let backends = Backends::default();
    let opened = backends
        .open(
            "file:notes?x=1",
            Path::new(BASE),
            LockSettings::default(),
            &system,
        )
        .await;
    let refused = StoreError::InvalidStore("`file:notes?x=1`: unknown option x".to_owned());
    assert_eq!(opened.err(), Some(refused));
}

#[test]
fn a_file_string_in_a_path_splits_at_its_root_and_keeps_its_query_on_it() {
    let splits = [
        ("file:notes/a.md", "file:.", "notes/a.md"),
        ("file:./notes/a.md", "file:.", "notes/a.md"),
        ("file:///srv/notes/a.md", "file:///", "srv/notes/a.md"),
        ("file:/srv/notes/a.md", "file:///", "srv/notes/a.md"),
        ("file:my%20notes/a.md?x=1", "file:.?x=1", "my notes/a.md"),
    ];
    for (text, container, inside) in splits {
        let split = FileBackend.split(text).unwrap();
        assert_eq!(split, (container.to_owned(), inside.to_owned()), "{text}");
    }
}

#[tokio::test]
async fn a_file_string_in_a_path_names_the_root_folder_and_the_path_inside_it() {
    let cases = [
        ("file:notes/a.md", "notes/a.md", "/v/notes/a.md"),
        ("file:./notes/a.md", "notes/a.md", "/v/notes/a.md"),
        (
            "file:///srv/notes/a.md",
            "srv/notes/a.md",
            "/srv/notes/a.md",
        ),
    ];
    for (text, inside, file) in cases {
        let system = MemorySystem::new();
        let backends = Backends::default();
        let (store, at) = backends
            .open_path(text, Path::new(BASE), LockSettings::default(), &system)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(at.as_str(), inside, "{text}");
        store
            .write(&at, b"# A\n", &Precondition::Absent)
            .await
            .unwrap();
        assert_eq!(
            system.read_to_string(Path::new(file)).unwrap(),
            "# A\n",
            "{text}"
        );
    }
}

#[tokio::test]
async fn a_store_string_in_a_path_with_nothing_after_its_container_is_an_invalid_store() {
    let system = MemorySystem::new();
    let backends = Backends::default();
    let base = Path::new(BASE);
    let lock = LockSettings::default();
    let refused = |text: &str| {
        Some(StoreError::InvalidStore(format!(
            "`{text}`: what follows the store is not a store path"
        )))
    };
    for text in ["file:.", "file:///", "file:notes/../../a.md"] {
        let opened = backends.open_path(text, base, lock, &system).await;
        assert_eq!(opened.err(), refused(text), "{text}");
    }
    let option = backends
        .open_path("file:notes/a.md?x=1", base, lock, &system)
        .await;
    let named = StoreError::InvalidStore("`file:notes/a.md?x=1`: unknown option x".to_owned());
    assert_eq!(option.err(), Some(named));
}

#[tokio::test]
async fn a_text_with_no_registered_scheme_is_a_plain_path_in_a_path_argument() {
    let system = MemorySystem::new();
    let backends = Backends::default();
    let plain = [
        "notes:report.md",
        "./a.md",
        "/srv/a.md",
        "a.md",
        "mailto:me",
        "C:/notes/a.md",
        "S3://acme/a.md",
        "${HOME}/a.md",
    ];
    for text in plain {
        let opened = backends
            .open_path(text, Path::new(BASE), LockSettings::default(), &system)
            .await
            .unwrap();
        assert!(opened.is_none(), "{text}");
    }
}

#[tokio::test]
async fn a_scheme_with_slashes_no_backend_answers_is_not_configured() {
    let system = MemorySystem::new();
    let backends = Backends::default();
    let base = Path::new(BASE);
    let lock = LockSettings::default();
    let refused = StoreError::NotConfigured(
        "`hdfs://namenode/vault`: no backend for hdfs in this build".to_owned(),
    );
    let opened = backends
        .open("hdfs://namenode/vault", base, lock, &system)
        .await;
    assert_eq!(opened.err(), Some(refused.clone()));
    let in_path = backends
        .open_path("hdfs://namenode/vault", base, lock, &system)
        .await;
    assert_eq!(in_path.err(), Some(refused));
}

#[test]
fn the_default_backends_answer_file_and_the_bucket_schemes_of_this_build() {
    let backends = Backends::default();
    assert!(backends.answering("file").is_some());
    assert_eq!(
        backends.answering("minio+http").is_some(),
        cfg!(feature = "minio")
    );
    assert_eq!(
        backends.answering("minio+https").is_some(),
        cfg!(feature = "minio")
    );
    assert_eq!(backends.answering("s3").is_some(), cfg!(feature = "s3"));
    assert!(backends.answering("hdfs").is_none());
}

#[tokio::test]
async fn a_backend_of_a_program_s_own_opens_its_scheme() {
    let system = MemorySystem::new();
    let backends = Backends::default().with(MemoryBackend);
    let base = Path::new(BASE);
    let lock = LockSettings::default();

    let store = backends
        .open("memory://x", base, lock, &system)
        .await
        .unwrap();
    store
        .write(&path("a.md"), b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        system.read_to_string(Path::new("/memory/x/a.md")).unwrap(),
        "# A\n"
    );

    let (in_path, at) = backends
        .open_path("memory://x/notes/b.md", base, lock, &system)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(at.as_str(), "notes/b.md");
    in_path
        .write(&at, b"# B\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        system
            .read_to_string(Path::new("/memory/x/notes/b.md"))
            .unwrap(),
        "# B\n"
    );

    let nameless = backends.open("memory://", base, lock, &system).await;
    let refused = StoreError::InvalidStore("`memory://`: no name".to_owned());
    assert_eq!(nameless.err(), Some(refused));
}

#[tokio::test]
async fn a_backend_added_for_a_built_in_scheme_answers_it_from_then_on() {
    let system = MemorySystem::new();
    let backends = Backends::default().with(OverFile);
    let store = backends
        .open(
            "file:notes",
            Path::new(BASE),
            LockSettings::default(),
            &system,
        )
        .await
        .unwrap();
    store
        .write(&path("a.md"), b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        system
            .read_to_string(Path::new("/overridden/a.md"))
            .unwrap(),
        "# A\n"
    );
}

#[tokio::test]
async fn within_a_folder_read_write_delete_and_read_embedded_take_the_folder_in_front() {
    let system = MemorySystem::new();
    let store = drafts(&system);
    let written = store
        .write(&path("a.md"), b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        system
            .read_to_string(Path::new("/notes/drafts/a.md"))
            .unwrap(),
        "# A\n"
    );
    let read = store.read(&path("a.md")).await.unwrap();
    assert_eq!(read.bytes, b"# A\n");
    assert_eq!(read.revision, written);

    store
        .write(&path("media/logo.png"), PNG, &Precondition::Absent)
        .await
        .unwrap();
    let embedded = store
        .read_embedded(&path("a.md"), &path("media/logo.png"))
        .await
        .unwrap();
    assert_eq!(embedded, PNG);

    store
        .delete(&path("a.md"), &Precondition::Matches(written))
        .await
        .unwrap();
    assert!(!system.exists(Path::new("/notes/drafts/a.md")).unwrap());
}

#[tokio::test]
async fn within_a_folder_a_save_puts_the_folder_in_front_of_the_document_and_each_file() {
    let system = MemorySystem::new();
    let store = drafts(&system);
    let files = BTreeMap::from([(path("posts/media/logo.png"), PNG.to_vec())]);
    store
        .write_with_embedded(&path("posts/a.md"), "# A\n", &files, &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        system
            .read_to_string(Path::new("/notes/drafts/posts/a.md"))
            .unwrap(),
        "# A\n"
    );
    assert!(
        system
            .exists(Path::new("/notes/drafts/posts/media/logo.png"))
            .unwrap()
    );
}

#[tokio::test]
async fn within_a_folder_a_listing_is_asked_under_the_folder_and_given_back_without_it() {
    let system = MemorySystem::new()
        .with_file("/notes/outside.md", b"# Outside\n")
        .unwrap();
    let store = drafts(&system);
    for name in ["a.md", "sub/b.md"] {
        store
            .write(&path(name), b"# A\n", &Precondition::Absent)
            .await
            .unwrap();
    }
    assert_eq!(
        store.list(None).await.unwrap(),
        [path("a.md"), path("sub/b.md")]
    );
    assert_eq!(
        store.list(Some(&path("sub"))).await.unwrap(),
        [path("sub/b.md")]
    );
}

#[tokio::test]
async fn within_a_folder_expired_locks_are_removed_under_the_folder_and_given_back_without_it() {
    let system = MemorySystem::new()
        .with_file("/notes/drafts/a.md.arky-lock", b"crashed")
        .unwrap()
        .with_file("/notes/drafts/sub/b.md.arky-lock", b"crashed")
        .unwrap()
        .with_file("/notes/outside.md.arky-lock", b"crashed")
        .unwrap();
    system.advance(Duration::from_secs(6)).unwrap();
    let store = drafts(&system);
    assert_eq!(
        store
            .remove_expired_locks(Some(&path("sub")))
            .await
            .unwrap(),
        [path("sub/b.md.arky-lock")]
    );
    assert_eq!(
        store.remove_expired_locks(None).await.unwrap(),
        [path("a.md.arky-lock")]
    );
    assert!(
        system
            .exists(Path::new("/notes/outside.md.arky-lock"))
            .unwrap()
    );
}

#[tokio::test]
async fn within_a_folder_capabilities_containers_and_deletions_pass_through() {
    let system = MemorySystem::new();
    let store = drafts(&system);
    assert_eq!(store.capabilities(), Capabilities::new(false));
    assert_eq!(
        store.ensure_container().await,
        Err(StoreError::Unsupported("create its container"))
    );
    assert_eq!(
        store.finish_deletions().await,
        Err(StoreError::Unsupported("finish deletions"))
    );
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_string_names_its_server_bucket_and_prefix_in_us_east_1_by_default() {
    let parsed = minio::parse("minio+https://minio.example.com/notes/vault").unwrap();
    let expected = Minio {
        bucket: "notes".to_owned(),
        endpoint: "https://minio.example.com".to_owned(),
        keys: None,
        prefix: Some(path("vault")),
        region: "us-east-1".to_owned(),
    };
    assert_eq!(parsed, expected);
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_string_with_keys_a_port_and_a_region_carries_them_decoded() {
    let text = "minio+http://AKIA0000:p%40ss%2Fword@127.0.0.1:9000/notes/a%20b/c?region=eu-west-1";
    let expected = Minio {
        bucket: "notes".to_owned(),
        endpoint: "http://127.0.0.1:9000".to_owned(),
        keys: Some(Keys {
            access: "AKIA0000".to_owned(),
            secret: PASSWORD.to_owned(),
        }),
        prefix: Some(path("a b/c")),
        region: "eu-west-1".to_owned(),
    };
    assert_eq!(minio::parse(text).unwrap(), expected);
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_string_with_no_bucket_an_unknown_option_or_a_fragment_is_an_invalid_store() {
    let refused = [
        ("minio+http://127.0.0.1:9000", "no bucket"),
        ("minio+http://127.0.0.1:9000/", "no bucket"),
        (
            "minio+http://127.0.0.1:9000/notes?regoin=eu-west-1",
            "unknown option regoin",
        ),
        (
            "minio+http://127.0.0.1:9000/notes#top",
            "a fragment, which a store string does not take",
        ),
        ("minio+http:///notes", "no host before the bucket"),
        (
            "minio+http://AKIA0000@127.0.0.1:9000/notes",
            "a key with no secret, or a secret with no key: the keys are key:secret@",
        ),
        ("minio+http://127.0.0.1:99999/notes", "invalid port number"),
    ];
    for (text, message) in refused {
        let expected = Err(StoreError::InvalidStore(message.to_owned()));
        assert_eq!(minio::parse(text), expected, "{text}");
    }
}

#[cfg(feature = "minio")]
#[tokio::test]
async fn a_minio_string_uses_its_keys_else_the_aws_variables() {
    let backends = Backends::default();
    let base = Path::new(BASE);
    let lock = LockSettings::default();
    let none_set = MemorySystem::new();
    let with_keys = "minio+http://AKIA0000:a-secret@127.0.0.1:9000/notes";
    backends
        .open(with_keys, base, lock, &none_set)
        .await
        .unwrap();

    let without = "minio+http://127.0.0.1:9000/notes";
    let refused = StoreError::Backend(
        "AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key".to_owned(),
    );
    let unset = backends.open(without, base, lock, &none_set).await;
    assert_eq!(unset.err(), Some(refused));
    let set = with_aws_keys();
    backends.open(without, base, lock, &set).await.unwrap();
}

#[cfg(feature = "minio")]
#[tokio::test]
async fn a_minio_string_in_a_path_names_the_bucket_and_the_path_inside_it() {
    let text = "minio+https://k:s@minio.example.com:9000/notes/vault/a.md?region=eu-west-1";
    let split = MinioBackend.split(text).unwrap();
    let container = "minio+https://k:s@minio.example.com:9000/notes?region=eu-west-1";
    assert_eq!(split, (container.to_owned(), "vault/a.md".to_owned()));
    assert_eq!(minio::parse(container).unwrap().prefix, None);

    let system = MemorySystem::new();
    let (_, at) = Backends::default()
        .open_path(text, Path::new(BASE), LockSettings::default(), &system)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(at.as_str(), "vault/a.md");
}

#[cfg(feature = "s3")]
#[test]
fn an_s3_string_names_its_bucket_as_the_host() {
    let expected = S3 {
        bucket: "acme-notes".to_owned(),
        keys: None,
        kms: None,
        prefix: Some(path("vault")),
        region: "eu-west-1".to_owned(),
    };
    let parsed = s3::parse("s3://acme-notes/vault?region=eu-west-1").unwrap();
    assert_eq!(parsed, expected);
}

#[cfg(feature = "s3")]
#[test]
fn an_s3_string_with_keys_and_a_kms_key_carries_them_decoded() {
    let kms = "arn:aws:kms:eu-west-1:111122223333:key/abcd";
    let text = format!("s3://AKIA%2Fuser:p%40ss%2Fword@acme-notes?region=eu-west-1&kms={kms}");
    let expected = S3 {
        bucket: "acme-notes".to_owned(),
        keys: Some(Keys {
            access: "AKIA/user".to_owned(),
            secret: PASSWORD.to_owned(),
        }),
        kms: Some(kms.to_owned()),
        prefix: None,
        region: "eu-west-1".to_owned(),
    };
    assert_eq!(s3::parse(&text).unwrap(), expected);
}

#[cfg(feature = "s3")]
#[test]
fn an_s3_string_with_no_region_a_port_or_an_unknown_option_is_an_invalid_store() {
    let refused = [
        (
            "s3://acme-notes/vault",
            "no region: s3 needs ?region=<region>",
        ),
        (
            "s3://acme-notes?region=",
            "no region: s3 needs ?region=<region>",
        ),
        (
            "s3://acme-notes:9000?region=eu-west-1",
            "a port, which s3 does not take: the bucket is the host",
        ),
        (
            "s3://acme?region=eu-west-1&regoin=x",
            "unknown option regoin",
        ),
        ("s3:///vault?region=eu-west-1", "no bucket"),
    ];
    for (text, message) in refused {
        let expected = Err(StoreError::InvalidStore(message.to_owned()));
        assert_eq!(s3::parse(text), expected, "{text}");
    }
}

#[cfg(feature = "s3")]
#[tokio::test]
async fn an_s3_string_s_invalid_store_names_the_string_as_written() {
    let system = with_aws_keys();
    let text = "s3://acme-notes/vault?regoin=eu-west-1";
    let backends = Backends::default();
    let wrong = backends
        .open(text, Path::new(BASE), LockSettings::default(), &system)
        .await;
    let refused = StoreError::InvalidStore(
        "`s3://acme-notes/vault?regoin=eu-west-1`: unknown option regoin".to_owned(),
    );
    assert_eq!(wrong.err(), Some(refused));
}

#[cfg(feature = "s3")]
#[tokio::test]
async fn an_s3_string_in_a_path_names_the_bucket_root_and_the_path_inside_it() {
    let text = "s3://acme-notes/vault/a.md?region=eu-west-1";
    let split = S3Backend.split(text).unwrap();
    let container = "s3://acme-notes?region=eu-west-1";
    assert_eq!(split, (container.to_owned(), "vault/a.md".to_owned()));
    assert_eq!(s3::parse(container).unwrap().prefix, None);

    let system = with_aws_keys();
    let (_, at) = Backends::default()
        .open_path(text, Path::new(BASE), LockSettings::default(), &system)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(at.as_str(), "vault/a.md");
}

#[cfg(feature = "s3")]
#[tokio::test]
async fn the_kms_key_of_an_s3_string_reaches_the_bucket_it_creates() {
    let kms = "arn:aws:kms:eu-west-1:111122223333:key/abcd";
    let parsed = s3::parse(&format!("s3://acme?region=eu-west-1&kms={kms}")).unwrap();
    let system = MemorySystem::new();
    let bucket = Recorded::default();
    let store = ObjectStore::new(bucket.clone(), None, &system);
    let finished = s3::finished(store, parsed.kms, LockSettings::default());
    Store::ensure_container(&finished).await.unwrap();
    assert_eq!(*bucket.kms.lock().unwrap(), [Some(kms.to_owned())]);
}
