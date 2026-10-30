//! The store over `MinioBucket` on the `RustFS` server `just test-stores`
//! starts, each test under a key prefix of its own.

use alloc::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use arky::bundle::{BundledDocument, Missing};
use arky::forms::{Form, encode};
use arky::parser::ast::RawDocument;
use arky::parser::{from_json, parse, to_json};
use arky::writer::{WriteOptions, write};
use os_shim::bucket::MinioBucket;
use os_shim::mock::MemorySystem;
use os_shim::real::RealSystem;

use super::{names, path};
use crate::any::AnyStore;
use crate::document::{get, put};
use crate::file::FileStore;
use crate::object::{ObjectConfig, ObjectStore};
use crate::{Precondition, Store as _, StoreError, StorePath};

const FIXTURES: [&str; 21] = [
    include_str!("../../../../arky/src/parser/tests/fixtures/api_reference.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/book_page.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/changelog.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/contact.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/daily_note.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/docusaurus_page.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/essay.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/hugo_post.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/hugo_post_with_table.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/issue_template.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/jekyll_post.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/meeting_notes.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/obsidian_note.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/pandoc_paper.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/readme.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/recipe.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/slash_command.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/task.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/toml_multi_line_string.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/yaml_list_of_mappings.md"),
    include_str!("../../../../arky/src/parser/tests/fixtures/zola_page.md"),
];

const FORMS: &[Form] = &[
    Form::Json,
    Form::Markdown,
    #[cfg(feature = "binary-format")]
    Form::Binary,
];

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The machine the stores of these tests run on.
static MACHINE: RealSystem = RealSystem::new();

/// The configuration `just test-stores` passes, its keys under `prefix`.
fn config(prefix: &str) -> ObjectConfig {
    ObjectConfig::new(
        env::var("ARKY_TEST_MINIO_ENDPOINT").unwrap(),
        "us-east-1".to_owned(),
        env::var("ARKY_TEST_MINIO_BUCKET").unwrap(),
        Some(path(prefix)),
    )
}

/// Gets the document at `path` from `from` in `form` and puts those bytes
/// at `to` in `into`.
async fn copy(
    from: &AnyStore<'_>,
    path: &StorePath,
    into: &AnyStore<'_>,
    to: &StorePath,
    form: Form,
) {
    let options = WriteOptions::default();
    let bundle = get(from, path, Missing::Keep).await.unwrap();
    let bytes = encode(&bundle, form, &options).unwrap();
    put(
        into,
        to,
        form,
        &bytes,
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap();
}

/// A store over the server, its keys read from the environment.
fn server(prefix: &str) -> ObjectStore<'static, MinioBucket> {
    ObjectStore::open(&MACHINE, config(prefix)).unwrap()
}

#[tokio::test]
async fn each_fixture_through_rustfs_and_back_is_arkys_form_and_a_second_copy_changes_nothing() {
    let minio = AnyStore::Minio(server("copy-contract"));
    let system = MemorySystem::new();
    let files = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &system));
    for (index, source) in FIXTURES.iter().enumerate() {
        let document = parse(source).document;
        let arky = write(&document, &WriteOptions::default()).unwrap();
        let kept: RawDocument = from_json(&to_json(&document).unwrap()).unwrap();
        let through_json = write(&kept, &WriteOptions::default()).unwrap();
        let fixture = path(&format!("fixtures/{index}.md"));
        files
            .write(&fixture, source.as_bytes(), &Precondition::None)
            .await
            .unwrap();
        for form in FORMS.iter().copied() {
            let first = path(&format!("{form:?}/{index}.md"));
            let second = path(&format!("{form:?}/again/{index}.md"));
            copy(&files, &fixture, &minio, &first, form).await;
            copy(&minio, &first, &files, &first, form).await;
            let back = files.read(&first).await.unwrap();
            let expected = if form == Form::Json {
                &through_json
            } else {
                &arky
            };
            assert_eq!(back.bytes, expected.as_bytes(), "{form:?}: {source}");
            copy(&files, &first, &minio, &second, form).await;
            copy(&minio, &second, &files, &second, form).await;
            let again = files.read(&second).await.unwrap();
            assert_eq!(again, back, "{form:?}: {source}");
            let stored = minio.read(&second).await.unwrap().bytes;
            assert_eq!(stored, back.bytes, "{form:?}: {source}");
        }
    }
}

#[tokio::test]
async fn ensure_creates_a_missing_bucket_and_leaves_it_alone_the_second_time() {
    let config = ObjectConfig::new(
        env::var("ARKY_TEST_MINIO_ENDPOINT").unwrap(),
        "us-east-1".to_owned(),
        "arky-ensure".to_owned(),
        None,
    );
    let store = ObjectStore::open(&MACHINE, config).unwrap();
    assert!(matches!(
        store.list(None).await,
        Err(StoreError::Backend(_))
    ));
    store.ensure().await.unwrap();
    let note = path("notes/a.md");
    store
        .write(&note, b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    store.ensure().await.unwrap();
    assert_eq!(names(&store.list(None).await.unwrap()), ["notes/a.md"]);
}

#[tokio::test]
async fn ensure_container_through_a_boxed_store_creates_a_missing_bucket_a_write_then_goes_to() {
    let config = ObjectConfig::new(
        env::var("ARKY_TEST_MINIO_ENDPOINT").unwrap(),
        "us-east-1".to_owned(),
        "arky-ensure-container".to_owned(),
        None,
    );
    let store: Box<dyn crate::DynStore> = Box::new(ObjectStore::open(&MACHINE, config).unwrap());
    assert!(matches!(
        store.list(None).await,
        Err(StoreError::Backend(_))
    ));
    store.ensure_container().await.unwrap();
    let note = path("notes/a.md");
    store
        .write(&note, b"# A\n", &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(names(&store.list(None).await.unwrap()), ["notes/a.md"]);
}

#[tokio::test]
async fn on_rustfs_a_put_keeps_each_embedded_file_beside_the_document_and_a_get_gives_it_back() {
    let store = server("embedded");
    let options = WriteOptions::default();
    let files = BTreeMap::from([("media/logo.png".to_owned(), PNG.to_vec())]);
    let bundle = BundledDocument::new(parse(REPORT).document, files, Missing::Refuse).unwrap();
    let zip = encode(&bundle, Form::Zip, &options).unwrap();
    let report = path("notes/report.md");
    put(
        &store,
        &report,
        Form::Zip,
        &zip,
        &options,
        &Precondition::None,
        Missing::Refuse,
    )
    .await
    .unwrap();
    let kept = store.list(None).await.unwrap();
    assert_eq!(names(&kept), ["notes/media/logo.png", "notes/report.md"]);
    let text = store.read(&report).await.unwrap().bytes;
    assert_eq!(text, REPORT.as_bytes());
    let logo = store
        .read(&path("notes/media/logo.png"))
        .await
        .unwrap()
        .bytes;
    assert_eq!(logo, PNG);
    assert_eq!(get(&store, &report, Missing::Refuse).await.unwrap(), bundle);
}

#[tokio::test]
async fn on_rustfs_a_precondition_that_does_not_hold_is_a_conflict_and_changes_nothing() {
    let store = server("preconditions");
    let task = path("notes/task.md");
    let first = store
        .write(&task, b"# Task\n", &Precondition::Absent)
        .await
        .unwrap();
    let read = store.read(&task).await.unwrap();
    assert_eq!(read.revision, first);
    assert!(
        first.0.starts_with('"') && first.0.ends_with('"'),
        "{first:?}"
    );
    let second = store
        .write(&task, b"# Done\n", &Precondition::Matches(read.revision))
        .await
        .unwrap();
    let kept = store.read(&task).await.unwrap();
    assert_eq!(kept.revision, second);

    let conflict = StoreError::Conflict(task.clone());
    let absent = store.write(&task, b"# B\n", &Precondition::Absent).await;
    assert_eq!(absent, Err(conflict.clone()));
    let stale = Precondition::Matches(first);
    let late = store.write(&task, b"# B\n", &stale).await;
    assert_eq!(late, Err(conflict.clone()));
    assert_eq!(store.delete(&task, &stale).await, Err(conflict.clone()));
    let held = store.delete(&task, &Precondition::Absent).await;
    assert_eq!(held, Err(conflict));
    assert_eq!(store.read(&task).await.unwrap(), kept);
    assert_eq!(names(&store.list(None).await.unwrap()), ["notes/task.md"]);

    store
        .delete(&task, &Precondition::Matches(second))
        .await
        .unwrap();
    let missing = StoreError::NotFound(task.clone());
    assert_eq!(store.read(&task).await, Err(missing));
    store.delete(&task, &Precondition::None).await.unwrap();
    store.delete(&task, &Precondition::Absent).await.unwrap();
    assert_eq!(store.list(None).await, Ok(Vec::new()));
}

#[tokio::test]
async fn on_rustfs_a_write_naming_a_revision_where_no_file_is_is_a_conflict_and_stores_nothing() {
    let store = server("no-file");
    let task = path("notes/task.md");
    let revision = store
        .write(&task, b"# Task\n", &Precondition::None)
        .await
        .unwrap();
    store.delete(&task, &Precondition::None).await.unwrap();
    let gone = Precondition::Matches(revision);
    let unmatched = store.write(&task, b"# Again\n", &gone).await;
    assert_eq!(unmatched, Err(StoreError::Conflict(task.clone())));
    let never = path("notes/never.md");
    let unknown = store.write(&never, b"# Never\n", &gone).await;
    assert_eq!(unknown, Err(StoreError::Conflict(never.clone())));
    assert_eq!(store.read(&task).await, Err(StoreError::NotFound(task)));
    assert_eq!(store.read(&never).await, Err(StoreError::NotFound(never)));
    assert_eq!(store.list(None).await, Ok(Vec::new()));
}

#[tokio::test]
async fn on_rustfs_a_listing_of_1001_files_gives_every_file_once_sorted() {
    let store = server("listing");
    let mut expected = Vec::new();
    for index in 0_u32..=1000_u32 {
        let file = path(&format!("notes/{index:04}.md"));
        store
            .write(&file, b"# File\n", &Precondition::None)
            .await
            .unwrap();
        expected.push(file);
    }
    let outside = path("notesx/a.md");
    store
        .write(&outside, b"# File\n", &Precondition::None)
        .await
        .unwrap();
    let notes = store.list(Some(&path("notes"))).await.unwrap();
    assert_eq!(notes, expected);
    expected.push(outside);
    assert_eq!(store.list(None).await.unwrap(), expected);
}

#[tokio::test]
async fn on_rustfs_a_request_signed_with_a_wrong_secret_key_fails_without_showing_it() {
    let secret = "arky-wrong-secret";
    let system = MemorySystem::new()
        .with_env("AWS_ACCESS_KEY_ID", "arky-test")
        .unwrap()
        .with_env("AWS_SECRET_ACCESS_KEY", secret)
        .unwrap();
    let store = ObjectStore::open(&system, config("wrong-secret")).unwrap();
    assert!(!format!("{store:?}").contains(secret));
    let task = path("notes/task.md");
    let errors = [
        store.read(&task).await.err(),
        store
            .write(&task, b"# Task\n", &Precondition::None)
            .await
            .err(),
        store.delete(&task, &Precondition::None).await.err(),
        store.list(None).await.err(),
    ];
    for error in errors {
        let shown = error.unwrap();
        assert!(matches!(shown, StoreError::Backend(_)), "{shown:?}");
        let message = shown.to_string();
        assert!(!message.contains(secret), "{message}");
    }
}
