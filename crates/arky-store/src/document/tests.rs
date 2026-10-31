//! `get`, `put` and `put_bundle` over `AnyStore::File`, each scenario on
//! `MemorySystem` and on `RealSystem` in a temporary folder.

use alloc::collections::BTreeMap;
use std::path::PathBuf;

use arky::bundle::{BundledDocument, BundledDocumentError, Missing};
#[cfg(feature = "binary-format")]
use arky::forms::{BINARY_MAGIC, BINARY_VERSION};
use arky::forms::{DecodeError, Form, FormError, encode};
use arky::parser::ast::{FrontmatterFormat, RawDocument};
use arky::parser::{from_json, parse, to_json};
use arky::writer::{WriteOptions, write};
use os_shim::System as _;
use os_shim::mock::MemorySystem;
use os_shim::real::RealSystem;
use tokio::runtime::Builder;

use super::{DocumentError, get, put, put_bundle};
use crate::any::AnyStore;
use crate::file::FileStore;
use crate::{Precondition, Revision, Store as _, StoreError, StorePath};

const FIXTURES: [&str; 21] = [
    include_str!("../../../arky/src/parser/tests/fixtures/api_reference.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/book_page.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/changelog.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/contact.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/daily_note.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/docusaurus_page.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/essay.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/hugo_post.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/hugo_post_with_table.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/issue_template.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/jekyll_post.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/meeting_notes.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/obsidian_note.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/pandoc_paper.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/readme.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/recipe.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/slash_command.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/task.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/toml_multi_line_string.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/yaml_list_of_mappings.md"),
    include_str!("../../../arky/src/parser/tests/fixtures/zola_page.md"),
];

const FORMS: &[Form] = &[
    Form::Json,
    Form::Markdown,
    #[cfg(feature = "binary-format")]
    Form::Binary,
];

const INTRO: &str = "Intro ![chart](../media/chart.png)\n";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The bundle of `text` and `files`, each a path in the bundle and its bytes.
fn bundle_of(text: &str, files: &[(&str, &[u8])]) -> BundledDocument {
    let held = files
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
        .collect();
    BundledDocument::new(parse(text).document, held, Missing::Refuse).unwrap()
}

/// The document at `path`, with every file it embeds, in `form`.
async fn in_form(
    store: &AnyStore<'_>,
    path: &StorePath,
    form: Form,
    options: &WriteOptions,
) -> Vec<u8> {
    let bundle = get(store, path, Missing::Keep).await.unwrap();
    encode(&bundle, form, options).unwrap()
}

fn names(paths: &[StorePath]) -> Vec<&str> {
    paths.iter().map(StorePath::as_str).collect()
}

/// Runs `scenario` over a store on each system.
fn on_both<S>(scenario: &S)
where
    S: AsyncFn(&AnyStore<'_>),
{
    let runtime = Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let memory = MemorySystem::new();
        let in_memory = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &memory));
        scenario(&in_memory).await;
        let real = RealSystem::new();
        let folder = real.create_temp_dir().await.unwrap();
        let on_disk = AnyStore::File(FileStore::new(folder.path().to_path_buf(), &real));
        scenario(&on_disk).await;
    });
}

/// Gets the document at `from` in `form` and puts those bytes at `to`.
async fn copy(store: &AnyStore<'_>, from: &StorePath, to: &StorePath, form: Form) -> Revision {
    let options = WriteOptions::default();
    let bytes = in_form(store, from, form, &options).await;
    put(
        store,
        to,
        form,
        &bytes,
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap()
}

/// Compiles only for a value that can go to another thread.
const fn sent<T>(value: T) -> T
where
    T: Send,
{
    value
}

#[tokio::test]
async fn the_sample_copies_a_list_through_json() {
    let system = MemorySystem::new();
    let store = AnyStore::File(FileStore::new("/vault".into(), &system));
    let options = WriteOptions::default();
    let a = StorePath::new("a.md").unwrap();
    let b = StorePath::new("b.md").unwrap();
    put(
        &store,
        &a,
        Form::Markdown,
        b"* item\n",
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap();
    let json = in_form(&store, &a, Form::Json, &options).await;
    put(
        &store,
        &b,
        Form::Json,
        &json,
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap();
    assert_eq!(store.read(&b).await.unwrap().bytes, b"- item\n");
}

async fn each_fixture_copied(store: &AnyStore<'_>) {
    for (index, source) in FIXTURES.iter().enumerate() {
        let document = parse(source).document;
        let arky = write(&document, &WriteOptions::default()).unwrap();
        let kept: RawDocument = from_json(&to_json(&document).unwrap()).unwrap();
        let through_json = write(&kept, &WriteOptions::default()).unwrap();
        let file = StorePath::new(&format!("fixtures/{index}.md")).unwrap();
        store
            .write(&file, source.as_bytes(), &Precondition::None)
            .await
            .unwrap();
        for form in FORMS.iter().copied() {
            let first = StorePath::new(&format!("{form:?}/{index}.md")).unwrap();
            let second = StorePath::new(&format!("{form:?}/again/{index}.md")).unwrap();
            let revision = copy(store, &file, &first, form).await;
            let copied = store.read(&first).await.unwrap().bytes;
            let expected = if form == Form::Json {
                &through_json
            } else {
                &arky
            };
            assert_eq!(copied, expected.as_bytes(), "{form:?}: {source}");
            assert_eq!(copy(store, &first, &second, form).await, revision);
            let again = store.read(&second).await.unwrap().bytes;
            assert_eq!(again, copied, "{form:?}: {source}");
        }
    }
}

#[test]
fn each_fixture_copied_in_each_form_is_arkys_form_and_a_second_copy_changes_nothing() {
    on_both(&each_fixture_copied);
}

async fn not_utf8(store: &AnyStore<'_>) {
    let path = StorePath::new("notes/a.md").unwrap();
    store
        .write(&path, b"a\xffb", &Precondition::None)
        .await
        .unwrap();
    let error = get(store, &path, Missing::Keep).await.unwrap_err();
    assert_eq!(error.to_string(), "notes/a.md: not UTF-8 at byte 1");
    assert!(
        matches!(
            &error,
            DocumentError::Form {
                path: failed,
                error: FormError::Decode(DecodeError::NotUtf8 { offset: 1 }),
            } if *failed == path
        ),
        "{error:?}"
    );
}

#[test]
fn a_get_of_a_file_that_is_not_utf8_names_the_path_and_the_offset() {
    on_both(&not_utf8);
}

async fn put_not_utf8(store: &AnyStore<'_>) {
    let path = StorePath::new("notes/a.md").unwrap();
    let options = WriteOptions::default();
    let refused = put(
        store,
        &path,
        Form::Markdown,
        b"a\xffb",
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.to_string(), "notes/a.md: not UTF-8 at byte 1");
    assert_eq!(store.list(None).await, Ok(Vec::new()));
}

#[test]
fn a_put_of_bytes_that_are_not_utf8_names_the_path_and_writes_nothing() {
    on_both(&put_not_utf8);
}

async fn absent_over_a_file(store: &AnyStore<'_>) {
    let path = StorePath::new("a.md").unwrap();
    store
        .write(&path, b"# A\n", &Precondition::None)
        .await
        .unwrap();
    let old = store.read(&path).await.unwrap();
    let refused = put(
        store,
        &path,
        Form::Markdown,
        b"# B\n",
        &WriteOptions::default(),
        &Precondition::Absent,
        Missing::Keep,
    )
    .await;
    assert!(
        matches!(
            &refused,
            Err(DocumentError::Store(StoreError::Conflict(conflict))) if *conflict == path
        ),
        "{refused:?}"
    );
    assert_eq!(store.read(&path).await.unwrap(), old);
}

#[test]
fn a_put_that_needs_no_file_over_a_file_is_a_conflict_and_changes_nothing() {
    on_both(&absent_over_a_file);
}

async fn toml_of_yaml(store: &AnyStore<'_>) {
    let source = include_str!("../../../arky/src/parser/tests/fixtures/jekyll_post.md");
    assert!(source.starts_with("---\n"));
    let path = StorePath::new("post.md").unwrap();
    store
        .write(&path, source.as_bytes(), &Precondition::None)
        .await
        .unwrap();
    let mut toml = WriteOptions::default();
    toml.frontmatter = FrontmatterFormat::Toml;
    let bytes = in_form(store, &path, Form::Markdown, &toml).await;
    assert!(bytes.starts_with(b"+++\n"));
    let expected = write(&parse(source).document, &toml).unwrap();
    assert_eq!(bytes, expected.as_bytes());
}

#[test]
fn a_get_with_toml_options_of_a_yaml_file_gives_toml_frontmatter() {
    on_both(&toml_of_yaml);
}

async fn above_the_folder(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let path = StorePath::new("notes/a.md").unwrap();
    let text = b"![Logo](../shared/logo.png)\n";
    put(
        store,
        &path,
        Form::Markdown,
        text,
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap();
    assert_eq!(in_form(store, &path, Form::Markdown, &options).await, text);
    for form in FORMS.iter().copied() {
        let copied = StorePath::new(&format!("{form:?}/a.md")).unwrap();
        copy(store, &path, &copied, form).await;
        assert_eq!(store.read(&copied).await.unwrap().bytes, text, "{form:?}");
    }
}

#[test]
fn a_put_then_a_get_of_an_embed_above_the_folder_gives_back_its_text() {
    on_both(&above_the_folder);
}

#[cfg(feature = "binary-format")]
async fn binary_copied(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let path = StorePath::new("a.md").unwrap();
    store
        .write(&path, b"* item\n", &Precondition::None)
        .await
        .unwrap();
    let binary = in_form(store, &path, Form::Binary, &options).await;
    assert_eq!(
        &binary[..6],
        [BINARY_MAGIC.as_slice(), &[BINARY_VERSION]].concat()
    );
    put(
        store,
        &path,
        Form::Binary,
        &binary,
        &options,
        &Precondition::None,
        Missing::Keep,
    )
    .await
    .unwrap();
    assert_eq!(store.read(&path).await.unwrap().bytes, b"- item\n");
}

#[cfg(feature = "binary-format")]
#[test]
fn a_get_in_binary_puts_back_as_arkys_form() {
    on_both(&binary_copied);
}

async fn each_form_beside(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let given = bundle_of(REPORT, &[("media/logo.png", PNG)]);
    for form in [
        #[cfg(feature = "binary-format")]
        Form::Binary,
        Form::Json,
        Form::Markdown,
        Form::Zip,
    ] {
        let bytes = encode(&given, form, &options).unwrap();
        let folder = StorePath::new(&format!("{form:?}")).unwrap();
        let path = StorePath::new(&format!("{form:?}/notes/report.md")).unwrap();
        put(
            store,
            &path,
            form,
            &bytes,
            &options,
            &Precondition::None,
            Missing::Refuse,
        )
        .await
        .unwrap();
        let kept = store.list(Some(&folder)).await.unwrap();
        let logo = path.beside("media/logo.png").unwrap();
        assert_eq!(kept, [logo.clone(), path.clone()], "{form:?}");
        let text = store.read(&path).await.unwrap().bytes;
        assert_eq!(text, REPORT.as_bytes(), "{form:?}");
        assert_eq!(store.read(&logo).await.unwrap().bytes, PNG, "{form:?}");
        let back = get(store, &path, Missing::Refuse).await.unwrap();
        assert_eq!(back.files(), ["media/logo.png"], "{form:?}");
        assert_eq!(back.file("media/logo.png"), Some(PNG), "{form:?}");
        assert_eq!(back, given, "{form:?}");
    }
}

#[test]
fn a_put_in_each_form_keeps_text_with_no_data_uri_and_files_beside_it_that_a_get_gives_back() {
    on_both(&each_form_beside);
}

async fn embedded_note(store: &AnyStore<'_>) {
    let report = StorePath::new("notes/report.md").unwrap();
    let files: [(&str, &[u8]); 4] = [
        ("notes/report.md", b"# Report\n\n![[parts/intro]]\n"),
        ("notes/parts/intro.md", INTRO.as_bytes()),
        ("notes/media/chart.png", PNG),
        ("notes/media/other.png", PNG),
    ];
    for (name, bytes) in files {
        let file = StorePath::new(name).unwrap();
        store
            .write(&file, bytes, &Precondition::None)
            .await
            .unwrap();
    }
    let back = get(store, &report, Missing::Refuse).await.unwrap();
    assert_eq!(back.files(), ["media/chart.png", "parts/intro.md"]);
    assert_eq!(back.file("media/chart.png"), Some(PNG));
    assert_eq!(back.file("parts/intro.md"), Some(INTRO.as_bytes()));
}

#[test]
fn a_get_gives_an_embedded_note_and_the_files_it_embeds_from_its_own_folder() {
    on_both(&embedded_note);
}

async fn file_gone(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let given = bundle_of(REPORT, &[("media/logo.png", PNG)]);
    let path = StorePath::new("notes/report.md").unwrap();
    let zip = encode(&given, Form::Zip, &options).unwrap();
    put(
        store,
        &path,
        Form::Zip,
        &zip,
        &options,
        &Precondition::None,
        Missing::Refuse,
    )
    .await
    .unwrap();
    let logo = StorePath::new("notes/media/logo.png").unwrap();
    store.delete(&logo, &Precondition::None).await.unwrap();
    let refused = get(store, &path, Missing::Refuse).await.unwrap_err();
    assert_eq!(
        refused.to_string(),
        "notes/report.md: index.md names media/logo.png, and the bundle does not hold it"
    );
    assert!(
        matches!(
            &refused,
            DocumentError::Form {
                path: failed,
                error: FormError::Decode(DecodeError::BundledDocument(
                    BundledDocumentError::Missing { target }
                )),
            } if *failed == path && target == "media/logo.png"
        ),
        "{refused:?}"
    );
    let kept = get(store, &path, Missing::Keep).await.unwrap();
    assert_eq!(kept.files(), Vec::<&str>::new());
    assert_eq!(kept.document(), given.document());
}

#[test]
fn a_get_of_a_document_whose_file_is_gone_fails_under_refuse_and_gives_no_file_under_keep() {
    on_both(&file_gone);
}

/// Writes `text`, whose one embed names `target`, at `notes/back.md` and checks
/// that a get gives the text unchanged and no file under `Keep`, and fails
/// naming `target` under `Refuse`.
async fn not_held(store: &AnyStore<'_>, text: &str, target: &str) {
    let path = StorePath::new("notes/back.md").unwrap();
    store
        .write(&path, text.as_bytes(), &Precondition::None)
        .await
        .unwrap();
    let kept = get(store, &path, Missing::Keep).await.unwrap();
    assert_eq!(kept.files(), Vec::<&str>::new());
    let written = write(kept.document(), &WriteOptions::default()).unwrap();
    assert_eq!(written, text);
    let refused = get(store, &path, Missing::Refuse).await.unwrap_err();
    assert_eq!(
        refused.to_string(),
        format!("notes/back.md: index.md names {target}, and the bundle does not hold it")
    );
    assert!(
        matches!(
            &refused,
            DocumentError::Form {
                path: failed,
                error: FormError::Decode(DecodeError::BundledDocument(
                    BundledDocumentError::Missing { target: named }
                )),
            } if *failed == path && named == target
        ),
        "{refused:?}"
    );
}

async fn backslash_missing(store: &AnyStore<'_>) {
    not_held(store, "![x](a\\b.png)\n", "a\\b.png").await;
}

#[test]
fn a_get_of_an_embed_written_with_a_backslash_whose_file_is_missing_gives_no_file_under_keep_and_fails_under_refuse()
 {
    on_both(&backslash_missing);
}

async fn a_folder(store: &AnyStore<'_>) {
    let inside = StorePath::new("notes/media/other.png").unwrap();
    store
        .write(&inside, PNG, &Precondition::None)
        .await
        .unwrap();
    not_held(store, "![x](media)\n", "media").await;
}

#[test]
fn a_get_of_an_embed_that_names_a_folder_gives_no_file_under_keep_and_fails_under_refuse() {
    on_both(&a_folder);
}

async fn put_backslash(store: &AnyStore<'_>) {
    let held = BTreeMap::from([("a\\b.png".to_owned(), PNG.to_vec())]);
    let refused = BundledDocument::new(parse("![x](a\\b.png)\n").document, held, Missing::Refuse);
    assert!(
        matches!(
            &refused,
            Err(BundledDocumentError::NotRelative { name }) if name == "a\\b.png"
        ),
        "{refused:?}"
    );
    let options = WriteOptions::default();
    let path = StorePath::new("notes/back.md").unwrap();
    let given = bundle_of("![x](a\\b.png)\n", &[("a/b.png", PNG)]);
    put_bundle(store, &path, &given, &options, &Precondition::None)
        .await
        .unwrap();
    assert_eq!(
        names(&store.list(None).await.unwrap()),
        ["notes/a/b.png", "notes/back.md"]
    );
    let file = StorePath::new("notes/a/b.png").unwrap();
    assert_eq!(store.read(&file).await.unwrap().bytes, PNG);
}

#[test]
fn a_bundle_cannot_hold_a_file_named_with_a_backslash_and_one_in_an_address_is_put_as_a_slash() {
    on_both(&put_backslash);
}

async fn backslash_beside(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let given = bundle_of("![x](media\\logo.png)\n", &[("media/logo.png", PNG)]);
    for form in [
        #[cfg(feature = "binary-format")]
        Form::Binary,
        Form::Json,
        Form::Markdown,
        Form::Zip,
    ] {
        let bytes = encode(&given, form, &options).unwrap();
        let path = StorePath::new(&format!("{form:?}/notes/report.md")).unwrap();
        put(
            store,
            &path,
            form,
            &bytes,
            &options,
            &Precondition::None,
            Missing::Refuse,
        )
        .await
        .unwrap();
        let logo = StorePath::new(&format!("{form:?}/notes/media/logo.png")).unwrap();
        assert_eq!(store.read(&logo).await.unwrap().bytes, PNG, "{form:?}");
        let back = get(store, &path, Missing::Refuse).await.unwrap();
        assert_eq!(back.files(), ["media/logo.png"], "{form:?}");
        assert_eq!(back.file("media/logo.png"), Some(PNG), "{form:?}");
    }
}

#[test]
fn a_put_in_each_form_of_an_address_holding_a_backslash_keeps_its_file_that_a_get_gives_back() {
    on_both(&backslash_beside);
}

async fn bundle_beside(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let text = "# Report\n\n![[parts/intro]]\n";
    let given = bundle_of(
        text,
        &[
            ("parts/intro.md", INTRO.as_bytes()),
            ("media/chart.png", PNG),
        ],
    );
    let path = StorePath::new("notes/report.md").unwrap();
    put_bundle(store, &path, &given, &options, &Precondition::Absent)
        .await
        .unwrap();
    assert_eq!(
        names(&store.list(None).await.unwrap()),
        [
            "notes/media/chart.png",
            "notes/parts/intro.md",
            "notes/report.md"
        ]
    );
    assert_eq!(store.read(&path).await.unwrap().bytes, text.as_bytes());
    let intro = StorePath::new("notes/parts/intro.md").unwrap();
    assert_eq!(store.read(&intro).await.unwrap().bytes, INTRO.as_bytes());
    let chart = StorePath::new("notes/media/chart.png").unwrap();
    assert_eq!(store.read(&chart).await.unwrap().bytes, PNG);
    assert_eq!(get(store, &path, Missing::Refuse).await.unwrap(), given);
}

#[test]
fn a_put_of_a_bundle_keeps_its_text_and_each_file_beside_it() {
    on_both(&bundle_beside);
}

async fn nothing_deleted(store: &AnyStore<'_>) {
    let options = WriteOptions::default();
    let path = StorePath::new("notes/report.md").unwrap();
    let old = bundle_of("![Old](media/old.png)\n", &[("media/old.png", PNG)]);
    let first = put_bundle(store, &path, &old, &options, &Precondition::Absent)
        .await
        .unwrap();
    let new = bundle_of("![New](media/new.png)\n", &[("media/new.png", b"new")]);
    let zip = encode(&new, Form::Zip, &options).unwrap();
    put(
        store,
        &path,
        Form::Zip,
        &zip,
        &options,
        &Precondition::Matches(first),
        Missing::Refuse,
    )
    .await
    .unwrap();
    assert_eq!(
        names(&store.list(None).await.unwrap()),
        [
            "notes/media/new.png",
            "notes/media/old.png",
            "notes/report.md"
        ]
    );
    assert_eq!(get(store, &path, Missing::Refuse).await.unwrap(), new);
}

#[test]
fn a_put_over_a_matching_revision_writes_the_new_files_and_deletes_none() {
    on_both(&nothing_deleted);
}

#[tokio::test]
async fn the_futures_of_get_put_and_put_bundle_over_any_store_can_be_sent_to_another_thread() {
    let system = MemorySystem::new();
    let store = AnyStore::File(FileStore::new(PathBuf::from("/vault"), &system));
    let options = WriteOptions::default();
    let path = StorePath::new("a.md").unwrap();
    let written = put(
        &store,
        &path,
        Form::Markdown,
        b"* item\n",
        &options,
        &Precondition::None,
        Missing::Keep,
    );
    sent(written).await.unwrap();
    let read = get(&store, &path, Missing::Keep);
    let bundle = sent(read).await.unwrap();
    assert_eq!(write(bundle.document(), &options).unwrap(), "- item\n");
    let again = put_bundle(&store, &path, &bundle, &options, &Precondition::None);
    sent(again).await.unwrap();
}
