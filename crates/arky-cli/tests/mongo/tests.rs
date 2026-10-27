use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command as Process, Stdio};

use arky::bundle::Missing;
use arky::forms::{Form, encode};
use arky::writer::WriteOptions;
use arky_store::any::AnyStore;
use arky_store::document::{get, put};
use arky_store::mongo::MongoStore;
use arky_store::object::{ObjectConfig, ObjectStore};
use arky_store::{Precondition, Store as _, StorePath};
use assert_cmd::Command;
use assert_cmd::assert::Assert;
use assert_cmd::cargo::cargo_bin;
use mongodb::bson::{Document, doc};
use mongodb::{Client, Collection};
use os_shim::real::RealSystem;
use predicates::ord::eq;
use tokio::runtime::{Builder, Runtime};

const DATABASE: &str = "arky_test_binary_copies";

const FINISHED_DATABASE: &str = "arky_test_binary_finished";

/// A prefix of its own, so its listing holds no other test's objects.
const FINISHED_PREFIX: &str = "binary/finished";

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../arky/src/parser/tests/fixtures"
);

const OTHER_PNG: &[u8] = b"\x89PNG\r\n\x1a\n another picture";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

/// The key prefix the binary's MongoDB store keeps its files under.
const PREFIX: &str = "binary/mongodb";

const REGION: &str = "us-east-1";

const REPLACED_DATABASE: &str = "arky_test_binary_replaced";

/// A prefix of its own, so its listing holds no other test's objects.
const REPLACED_PREFIX: &str = "binary/replaced";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The machine the stores of these tests run on.
static MACHINE: RealSystem = RealSystem::new();

/// Runs the binary in `root` with `arguments`, its MongoDB store's files
/// under `prefix` in the bucket.
fn arky_on(root: &Path, prefix: &str, arguments: &[&str]) -> Assert {
    Command::cargo_bin("arky")
        .unwrap()
        .current_dir(root)
        .args(arguments)
        .env("ARKY_MONGODB_URI", uri())
        .env("ARKY_MONGODB_BUCKET", format!("{}/{prefix}", bucket()))
        .env("ARKY_MINIO_ENDPOINT", endpoint())
        .env("ARKY_MINIO_REGION", REGION)
        .assert()
}

/// The bucket `just test-stores` makes.
fn bucket() -> String {
    env::var("ARKY_TEST_MINIO_BUCKET").unwrap()
}

/// The server's address `just test-stores` sets.
fn endpoint() -> String {
    env::var("ARKY_TEST_MINIO_ENDPOINT").unwrap()
}

/// The 21 markdown fixture files of arky, by name.
fn fixtures() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = fs::read_dir(FIXTURES)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .map(|path| {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            (name, fs::read_to_string(&path).unwrap())
        })
        .collect();
    files.sort_unstable();
    assert_eq!(files.len(), 21);
    files
}

/// arky's form of `source` as the MongoDB store keeps it, an offset written
/// as UTC: put at `name` in `expected` and got back, through the library.
fn kept_by_mongodb(
    runtime: &Runtime,
    expected: &AnyStore<'_>,
    name: &str,
    source: &str,
) -> Vec<u8> {
    let path = StorePath::new(name).unwrap();
    let options = WriteOptions::default();
    runtime.block_on(async {
        put(
            expected,
            &path,
            Form::Markdown,
            source.as_bytes(),
            &options,
            &Precondition::None,
            Missing::Keep,
        )
        .await
        .unwrap();
        let bundle = get(expected, &path, Missing::Keep).await.unwrap();
        encode(&bundle, Form::Markdown, &options).unwrap()
    })
}

/// Runs `arky get name` in `root` piped into `arky write name --store
/// store`, both with `--allow-missing`, and asserts both runs succeeded.
fn pipe(root: &Path, name: &str, store: &str) {
    let mut get = Process::new(cargo_bin("arky"))
        .current_dir(root)
        .args(["get", name, "--allow-missing"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let written = Process::new(cargo_bin("arky"))
        .current_dir(root)
        .args(["write", name, "--store", store, "--allow-missing"])
        .env("ARKY_MONGODB_URI", uri())
        .env("ARKY_MONGODB_BUCKET", format!("{}/{PREFIX}", bucket()))
        .env("ARKY_MINIO_ENDPOINT", endpoint())
        .env("ARKY_MINIO_REGION", REGION)
        .stdin(get.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(get.wait().unwrap().success(), "{name}");
    assert!(written.success(), "{name}");
}

/// The connection string `just test-stores` sets.
fn uri() -> String {
    env::var("ARKY_TEST_MONGODB_URI").unwrap()
}

#[test]
fn each_fixture_piped_into_mongodb_and_read_back_is_arkys_form_as_mongodb_keeps_it() {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let prefix = StorePath::new("binary/expected").unwrap();
    let config = ObjectConfig::new(endpoint(), REGION.to_owned(), bucket(), Some(prefix));
    let files = ObjectStore::open(&MACHINE, config).unwrap();
    let opened = runtime.block_on(MongoStore::open(&uri(), DATABASE, "expected", files));
    let expected = AnyStore::Mongo(opened.unwrap());
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = format!("mongodb:{DATABASE}/files");
    // `obsidian_note.md` embeds a picture this folder does not hold.
    for (name, source) in fixtures() {
        fs::write(root.join(&name), &source).unwrap();
        pipe(root, &name, &store);
        let kept = kept_by_mongodb(&runtime, &expected, &name, &source);
        Command::cargo_bin("arky")
            .unwrap()
            .current_dir(root)
            .args(["get", &name, "--store", &store, "--allow-missing"])
            .env("ARKY_MONGODB_URI", uri())
            .env("ARKY_MONGODB_BUCKET", format!("{}/{PREFIX}", bucket()))
            .env("ARKY_MINIO_ENDPOINT", endpoint())
            .env("ARKY_MINIO_REGION", REGION)
            .assert()
            .success()
            .stdout(eq(kept.as_slice()));
    }
    runtime.block_on(async {
        let client = Client::with_uri_str(uri()).await.unwrap();
        client.database(DATABASE).drop().await.unwrap();
    });
}

#[test]
fn asset_finish_deletions_finishes_a_deletion_that_stopped_and_a_write_of_its_bytes_then_succeeds()
{
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = format!("mongodb:{FINISHED_DATABASE}/files");
    fs::create_dir_all(root.join("draft/media")).unwrap();
    fs::write(root.join("draft/report.md"), REPORT).unwrap();
    fs::write(root.join("draft/media/logo.png"), PNG).unwrap();
    fs::write(root.join("draft/plain.md"), "# Report\n").unwrap();
    let client = runtime.block_on(Client::with_uri_str(uri())).unwrap();
    let records: Collection<Document> =
        client.database(FINISHED_DATABASE).collection("files_files");
    let embedding = [
        "write",
        "notes/report.md",
        "-i",
        "draft/report.md",
        "--store",
        &store,
    ];
    arky_on(root, FINISHED_PREFIX, &embedding).success();
    let stored = runtime.block_on(async { records.find_one(doc! {}).await.unwrap().unwrap() });
    let hash = stored.get_str("_id").unwrap();
    let plain = [
        "write",
        "notes/report.md",
        "-i",
        "draft/plain.md",
        "--store",
        &store,
    ];
    arky_on(root, FINISHED_PREFIX, &plain).success();
    let left = runtime.block_on(async { records.count_documents(doc! {}).await.unwrap() });
    assert_eq!(left, 0);

    let object = format!("files/{hash}");
    let held = doc! {
        "_id": hash,
        "object": object.as_str(),
        "content_type": "image/png",
        "size": 27_i64,
        "description": "Logo",
        "stored": true,
        "deleting": true,
        "embedded_by": [],
    };
    runtime.block_on(async { records.insert_one(held).await.unwrap() });
    let finish = ["asset", "finish-deletions", "--store", &store];
    let line = format!("{object}\n");
    arky_on(root, FINISHED_PREFIX, &finish)
        .success()
        .stdout(eq(line.as_bytes()));

    let other = [
        "write",
        "notes/other.md",
        "-i",
        "draft/report.md",
        "--store",
        &store,
    ];
    arky_on(root, FINISHED_PREFIX, &other).success();
    let get = [
        "get",
        "notes/other.md",
        "-o",
        "out/other.md",
        "--store",
        &store,
    ];
    arky_on(root, FINISHED_PREFIX, &get).success();
    assert_eq!(fs::read(root.join("out/media/logo.png")).unwrap(), PNG);
    runtime.block_on(async { client.database(FINISHED_DATABASE).drop().await.unwrap() });
}

#[test]
fn a_second_write_with_a_new_picture_deletes_the_old_object_and_its_record_and_get_reads_the_new_one()
 {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = format!("mongodb:{REPLACED_DATABASE}/files");
    fs::create_dir_all(root.join("draft/media")).unwrap();
    fs::write(root.join("draft/report.md"), REPORT).unwrap();
    for picture in [PNG, OTHER_PNG] {
        fs::write(root.join("draft/media/logo.png"), picture).unwrap();
        let write = [
            "write",
            "notes/report.md",
            "-i",
            "draft/report.md",
            "--store",
            &store,
        ];
        arky_on(root, REPLACED_PREFIX, &write).success();
    }
    let get = [
        "get",
        "notes/report.md",
        "-o",
        "out/report.md",
        "--store",
        &store,
    ];
    arky_on(root, REPLACED_PREFIX, &get).success();
    assert_eq!(
        fs::read(root.join("out/media/logo.png")).unwrap(),
        OTHER_PNG
    );
    runtime.block_on(async {
        let prefix = StorePath::new(REPLACED_PREFIX).unwrap();
        let config = ObjectConfig::new(endpoint(), REGION.to_owned(), bucket(), Some(prefix));
        let files = ObjectStore::open(&MACHINE, config).unwrap();
        let objects = files.list(None).await.unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(files.read(&objects[0]).await.unwrap().bytes, OTHER_PNG);
        let client = Client::with_uri_str(uri()).await.unwrap();
        let records: Collection<Document> =
            client.database(REPLACED_DATABASE).collection("files_files");
        assert_eq!(records.count_documents(doc! {}).await.unwrap(), 1);
        let record = records.find_one(doc! {}).await.unwrap().unwrap();
        assert_eq!(record.get_str("object").unwrap(), objects[0].as_str());
        client.database(REPLACED_DATABASE).drop().await.unwrap();
    });
}
