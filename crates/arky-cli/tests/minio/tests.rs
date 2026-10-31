use core::time::Duration;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::sync::LazyLock;
use std::time::SystemTime;

use arky::parser::parse;
use arky::writer::{WriteOptions, write};
use arky_store::object::{ObjectConfig, ObjectStore};
use arky_store::{Precondition, Store as _, StoreError, StorePath};
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin;
use os_shim::real::RealSystem;
use predicates::ord::eq;
use predicates::str::{is_empty, starts_with};
use tokio::runtime::Builder;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../arky/src/parser/tests/fixtures"
);

/// The key prefix the copies are under.
const PREFIX: &str = "binary/copies";

const REGION: &str = "us-east-1";

/// The machine the stores of these tests run on.
static MACHINE: LazyLock<RealSystem> = LazyLock::new(RealSystem::new);

/// `arky` run in `root`.
fn arky(root: &Path) -> Command {
    let mut process = Process::new(cargo_bin("arky"));
    isolated(&mut process, root).current_dir(root);
    Command::from_std(process)
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

/// `command` cut off from the machine's configuration: no `ARKY_STORE`, and
/// `ARKY_CONFIG` naming an empty `arky.toml` in `folder`, so no `arky.toml`
/// above the test's folder is read.
fn isolated<'command>(command: &'command mut Process, folder: &Path) -> &'command mut Process {
    let config = folder.join("isolated-arky.toml");
    fs::write(&config, "").unwrap();
    command.env_remove("ARKY_STORE").env("ARKY_CONFIG", &config)
}

/// Runs `arky get name` in `root` piped into `arky write name --store
/// store`, both with `--allow-missing`, and asserts both runs succeeded.
fn pipe(root: &Path, name: &str, store: &str) {
    let mut get = isolated(&mut Process::new(cargo_bin("arky")), root)
        .current_dir(root)
        .args(["get", name, "--allow-missing"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let written = isolated(&mut Process::new(cargo_bin("arky")), root)
        .current_dir(root)
        .args(["write", name, "--store", store, "--allow-missing"])
        .stdin(get.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(get.wait().unwrap().success(), "{name}");
    assert!(written.success(), "{name}");
}

/// The store string of `prefix` in the bucket `just test-stores` makes, its
/// keys read from the environment.
fn store(prefix: &str) -> String {
    format!("minio+{}/{}/{prefix}", endpoint(), bucket())
}

#[test]
fn each_fixture_piped_into_rustfs_and_back_to_the_file_system_is_arkys_form() {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let config = ObjectConfig::new(endpoint(), REGION.to_owned(), bucket(), None);
    let whole_bucket = ObjectStore::open(&*MACHINE, config).unwrap();
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = store(PREFIX);
    // `obsidian_note.md` embeds a picture this folder does not hold.
    for (name, source) in fixtures() {
        let arky_form = write(&parse(&source).document, &WriteOptions::default()).unwrap();
        fs::write(root.join(&name), &source).unwrap();
        pipe(root, &name, &store);
        let key = StorePath::new(&format!("{PREFIX}/{name}")).unwrap();
        let kept = runtime.block_on(whole_bucket.read(&key)).unwrap();
        assert_eq!(kept.bytes, arky_form.as_bytes(), "{name}");
        let got = arky(root)
            .args(["get", &name, "--store", &store, "--allow-missing"])
            .assert()
            .success()
            .stdout(eq(arky_form.as_bytes()));
        let back = format!("back/{name}");
        arky(root)
            .args(["write", &back, "--allow-missing"])
            .write_stdin(got.get_output().stdout.clone())
            .assert()
            .success();
        let copied = fs::read_to_string(root.join(&back)).unwrap();
        assert_eq!(copied, arky_form, "{name}");
    }
}

#[test]
fn a_request_signed_with_a_wrong_secret_key_exits_4_without_showing_it() {
    let secret = "arky-binary-wrong-secret";
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = store("binary/wrong-secret");
    let named = format!("arky: `{store}`: ");
    for (command, line) in [("get", "arky: a.md: "), ("write", named.as_str())] {
        let ran = arky(root)
            .args([command, "a.md", "--store", &store])
            .env("AWS_SECRET_ACCESS_KEY", secret)
            .write_stdin(b"# A\n".as_slice())
            .assert()
            .code(4_i32)
            .stdout(is_empty())
            .stderr(starts_with(line));
        let stderr = String::from_utf8(ran.get_output().stderr.clone()).unwrap();
        assert!(!stderr.contains(secret), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn a_write_to_a_missing_bucket_creates_it_and_a_get_reads_the_document_back() {
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let store = format!("minio+{}/arky-cli-created/notes", endpoint());
    arky(root)
        .args(["write", "a.md", "--store", &store])
        .write_stdin(b"# A\n".as_slice())
        .assert()
        .success();
    arky(root)
        .args(["get", "a.md", "--store", &store])
        .assert()
        .success()
        .stdout(eq(b"# A\n".as_slice()));
}

#[test]
fn fix_removes_the_expired_lock_object_under_a_path_and_leaves_the_fresh_one_and_other_objects() {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let config = ObjectConfig::new(endpoint(), REGION.to_owned(), bucket(), None);
    let whole_bucket = ObjectStore::open(&*MACHINE, config).unwrap();
    let ahead = SystemTime::now() + Duration::from_secs(3600);
    let seconds = ahead
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let objects = [
        ("old.md.arky-lock", "another 1000".to_owned()),
        ("new.md.arky-lock", format!("another {seconds}")),
        ("Cargo.lock", "old".to_owned()),
    ];
    for (name, bytes) in &objects {
        let key = StorePath::new(&format!("binary/fix/notes/{name}")).unwrap();
        runtime
            .block_on(whole_bucket.write(&key, bytes.as_bytes(), &Precondition::None))
            .unwrap();
    }
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = store("binary/fix");
    arky(folder.path())
        .args(["fix", "notes", "--store", &store])
        .assert()
        .success()
        .stdout(eq("notes/old.md.arky-lock\n"))
        .stderr(is_empty());
    let read = |name: &str| {
        let key = StorePath::new(&format!("binary/fix/notes/{name}")).unwrap();
        runtime.block_on(whole_bucket.read(&key))
    };
    assert!(matches!(
        read("old.md.arky-lock"),
        Err(StoreError::NotFound(_))
    ));
    assert_eq!(
        read("new.md.arky-lock").unwrap().bytes,
        objects[1].1.as_bytes()
    );
    assert_eq!(read("Cargo.lock").unwrap().bytes, b"old");
}

#[test]
fn a_folder_after_the_bucket_in_a_store_string_or_a_saved_name_is_a_folder_inside_it() {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let config = ObjectConfig::new(endpoint(), REGION.to_owned(), bucket(), None);
    let whole_bucket = ObjectStore::open(&*MACHINE, config).unwrap();
    let key = StorePath::new("binary/folder/drafts/ideas.md").unwrap();
    runtime
        .block_on(whole_bucket.write(&key, b"# Ideas\n", &Precondition::None))
        .unwrap();
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    let saved = format!("[stores.bucket]\nurl = \"{}\"\n", store("binary/folder"));
    fs::write(root.join("arky.toml"), saved).unwrap();
    let in_store = store("binary/folder/drafts");
    let in_path = format!("{}/ideas.md", store("binary/folder/drafts"));
    for arguments in [
        ["get", "ideas.md", "-s", in_store.as_str()].as_slice(),
        ["get", "ideas.md", "-s", "arky://bucket/drafts"].as_slice(),
        ["get", "drafts/ideas.md", "-s", "arky://bucket"].as_slice(),
        ["get", "arky://bucket/drafts/ideas.md"].as_slice(),
        ["get", in_path.as_str()].as_slice(),
    ] {
        Command::cargo_bin("arky")
            .unwrap()
            .current_dir(root)
            .args(arguments)
            .env_remove("ARKY_STORE")
            .env_remove("ARKY_CONFIG")
            .assert()
            .success()
            .stdout(eq(b"# Ideas\n".as_slice()));
    }
}
