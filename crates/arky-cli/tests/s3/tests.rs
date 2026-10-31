use std::env;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

use arky_store::object::{ObjectStore, S3ObjectConfig};
use arky_store::{Precondition, Store as _, StorePath};
use assert_cmd::Command;
use os_shim::real::RealSystem;
use predicates::str::is_empty;
use tokio::runtime::Builder;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// The machine the store of these tests runs on.
static MACHINE: LazyLock<RealSystem> = LazyLock::new(RealSystem::new);

/// `arky` run in `root`.
fn arky(root: &Path) -> Command {
    let mut command = Command::cargo_bin("arky").unwrap();
    isolated(&mut command, root).current_dir(root);
    command
}

/// `command` cut off from the machine's configuration: no `ARKY_STORE`, and
/// `ARKY_CONFIG` naming an empty `arky.toml` in `folder`, so no `arky.toml`
/// above the test's folder is read.
fn isolated<'command>(command: &'command mut Command, folder: &Path) -> &'command mut Command {
    let config = folder.join("isolated-arky.toml");
    fs::write(&config, "").unwrap();
    command.env_remove("ARKY_STORE").env("ARKY_CONFIG", &config)
}

#[test]
fn a_document_with_a_picture_written_to_s3_reads_back_whole_and_fix_finds_no_lock() {
    let since = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let prefix = format!("arky-cli-tests/{}", since.as_nanos());
    let bucket = env::var("ARKY_TEST_S3_BUCKET").unwrap();
    let region = env::var("ARKY_TEST_S3_REGION").unwrap();
    let store = format!("s3://{bucket}/{prefix}?region={region}");
    let folder = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = folder.path();
    fs::create_dir_all(root.join("draft/media")).unwrap();
    fs::write(root.join("draft/report.md"), REPORT).unwrap();
    fs::write(root.join("draft/media/logo.png"), PNG).unwrap();
    arky(root)
        .args(["write", "notes/report.md", "-i", "draft/report.md"])
        .args(["-s", &store])
        .assert()
        .success()
        .stdout(is_empty())
        .stderr(is_empty());
    arky(root)
        .args(["get", "notes/report.md", "-o", "copy/report.md"])
        .args(["-s", &store])
        .assert()
        .success()
        .stdout(is_empty())
        .stderr(is_empty());
    assert_eq!(
        fs::read_to_string(root.join("copy/report.md")).unwrap(),
        REPORT
    );
    assert_eq!(fs::read(root.join("copy/media/logo.png")).unwrap(), PNG);
    arky(root)
        .args(["fix", "-s", &store])
        .assert()
        .success()
        .stdout(is_empty())
        .stderr(is_empty());

    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let config = S3ObjectConfig::new(region, bucket, Some(StorePath::new(&prefix).unwrap()));
    let objects = ObjectStore::open_s3(&*MACHINE, config).unwrap();
    runtime.block_on(async {
        for path in objects.list(None).await.unwrap() {
            objects.delete(&path, &Precondition::None).await.unwrap();
        }
        assert_eq!(objects.list(None).await.unwrap(), Vec::<StorePath>::new());
    });
}
