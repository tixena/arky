//! `run` over `MemorySystem`, with byte buffers for the standard streams.

use core::iter;
use core::time::Duration;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use arky::bundle::Missing;
use arky::forms::{Form, decode};
use arky::parser::ast::{FrontmatterFormat, RawDocument};
use arky::parser::{from_json, parse, to_json};
use arky::writer::{WriteOptions, write};
use clap::Parser as _;
use os_shim::System as _;
use os_shim::mock::MemorySystem;
use tokio::runtime::Builder;

use super::run;
use crate::cli::Cli;

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

const FORMS: &[&str] = &[
    "markdown",
    "json",
    #[cfg(feature = "binary-format")]
    "binary",
    "zip",
];

const JEKYLL_POST: &str = FIXTURES[10];

const LOCK: &str = "/vault/notes/report.md.arky-lock";

/// The four variables a `MinIO` store reads: a server no process listens on,
/// whose connection is refused at once, and keys no output may show.
#[cfg(any(feature = "minio", feature = "mongodb"))]
const MINIO_VARIABLES: [(&str, &str); 4] = [
    ("ARKY_MINIO_ENDPOINT", "http://127.0.0.1:1"),
    ("ARKY_MINIO_REGION", "us-east-1"),
    ("AWS_ACCESS_KEY_ID", "arky-unit-access"),
    ("AWS_SECRET_ACCESS_KEY", "arky-unit-secret"),
];

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

const SAVE: [&str; 4] = ["write", "notes/report.md", "-i", "draft/report.md"];

const START: u64 = 1_792_700_000;

/// A server no process listens on, which the driver tries once and gives up
/// on at once: no server selection to wait for, no retry.
#[cfg(feature = "mongodb")]
const UNREACHABLE: &str = "mongodb://127.0.0.1:1/?loadBalanced=true&retryReads=false";

const ZOLA_PAGE: &str = FIXTURES[20];

/// What a run gave: its exit code and the two output streams.
struct Ran {
    code: ExitCode,
    stderr: String,
    stdout: Vec<u8>,
}

/// Runs `arky` and `arguments` over `system`, `stdin` on standard input.
fn arky(system: &MemorySystem, arguments: &[&str], stdin: &[u8]) -> Ran {
    let cli = Cli::try_parse_from(iter::once("arky").chain(arguments.iter().copied())).unwrap();
    let mut input = stdin;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let code = runtime.block_on(run(&cli, system, &mut input, &mut stdout, &mut stderr));
    Ran {
        code,
        stderr: String::from_utf8(stderr).unwrap(),
        stdout,
    }
}

/// arky's form of the markdown `source`, its frontmatter in `frontmatter`.
fn arkys_form(source: &str, frontmatter: FrontmatterFormat) -> String {
    let mut options = WriteOptions::default();
    options.frontmatter = frontmatter;
    write(&parse(source).document, &options).unwrap()
}

/// arky's form of what the JSON form keeps of the markdown `source`: its
/// date-times in UTC.
fn arkys_form_through_json(source: &str) -> String {
    let json = to_json(&parse(source).document).unwrap();
    let kept: RawDocument = from_json(&json).unwrap();
    write(&kept, &WriteOptions::default()).unwrap()
}

/// The bytes of the file at `path` in `system`.
fn bytes_of(system: &MemorySystem, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    system
        .open(Path::new(path))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

/// Asserts `ran` failed with `code` and one line on standard error that
/// opens with `line`, and wrote nothing to standard output.
fn failed(ran: &Ran, code: u8, line: &str) {
    assert_eq!(ran.code, ExitCode::from(code), "{}", ran.stderr);
    assert!(ran.stderr.starts_with(line), "{}", ran.stderr);
    assert!(ran.stderr.ends_with('\n'), "{}", ran.stderr);
    assert_eq!(ran.stderr.lines().count(), 1, "{}", ran.stderr);
    assert_eq!(ran.stdout, b"");
}

/// Asserts neither key of `MINIO_VARIABLES` is on `ran`'s standard error.
#[cfg(any(feature = "minio", feature = "mongodb"))]
fn hides_the_keys(ran: &Ran) {
    for (_, key) in &MINIO_VARIABLES[2..] {
        assert!(!ran.stderr.contains(key), "{}", ran.stderr);
    }
}

/// `report()` on a clock at `START`, with `files` besides, and a lock of
/// another save beside `notes/report.md` taken `ago` before `START`'s moment.
fn locked(ago: u64, files: &[(&str, &str)]) -> MemorySystem {
    let system = MemorySystem::new()
        .with_now(started() - Duration::from_secs(ago))
        .unwrap()
        .with_file(LOCK, b"another")
        .unwrap()
        .with_file("/vault/draft/report.md", REPORT.as_bytes())
        .unwrap()
        .with_file("/vault/draft/media/logo.png", PNG)
        .unwrap()
        .with_current_dir("/vault")
        .unwrap();
    system.advance(Duration::from_secs(ago)).unwrap();
    files.iter().fold(system, |with, (path, text)| {
        with.with_file(path, text.as_bytes()).unwrap()
    })
}

/// `system` with each of `MINIO_VARIABLES` but the one named `left_out`.
#[cfg(any(feature = "minio", feature = "mongodb"))]
fn minio_variables(system: MemorySystem, left_out: Option<&str>) -> MemorySystem {
    MINIO_VARIABLES
        .iter()
        .filter(|(name, _)| Some(*name) != left_out)
        .fold(system, |with, (name, value)| {
            with.with_env(name, value).unwrap()
        })
}

/// `system` with what a MongoDB store reads: `UNREACHABLE`, a bucket, and each
/// of `MINIO_VARIABLES` but the one named `left_out`.
#[cfg(feature = "mongodb")]
fn mongodb_variables(system: MemorySystem, left_out: Option<&str>) -> MemorySystem {
    let with = system
        .with_env("ARKY_MONGODB_URI", UNREACHABLE)
        .unwrap()
        .with_env("ARKY_MONGODB_BUCKET", "arky-files")
        .unwrap();
    minio_variables(with, left_out)
}

fn read(system: &MemorySystem, path: &str) -> String {
    system.read_to_string(Path::new(path)).unwrap()
}

fn since_start(system: &MemorySystem) -> Duration {
    system.now().duration_since(started()).unwrap()
}

fn started() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(START)
}

/// A system whose current folder is `/vault`, holding `notes/report.md`, which
/// embeds `media/logo.png`, and that picture beside it.
fn report() -> MemorySystem {
    MemorySystem::new()
        .with_file("/vault/notes/report.md", REPORT.as_bytes())
        .unwrap()
        .with_file("/vault/notes/media/logo.png", PNG)
        .unwrap()
        .with_current_dir("/vault")
        .unwrap()
}

/// Asserts `system` holds `REPORT` at `path` and `PNG` beside it at
/// `media/logo.png`.
fn reported(system: &MemorySystem, path: &str) {
    assert_eq!(read(system, path), REPORT, "{path}");
    let folder = Path::new(path).parent().unwrap();
    let logo = folder.join("media/logo.png");
    assert_eq!(bytes_of(system, logo.to_str().unwrap()), PNG, "{path}");
}

/// A system whose current folder is `/vault`, holding `a.md` with `source`.
fn vault(source: &str) -> MemorySystem {
    MemorySystem::new()
        .with_file("/vault/a.md", source.as_bytes())
        .unwrap()
        .with_current_dir("/vault")
        .unwrap()
}

#[test]
fn each_fixture_copied_in_each_form_is_arkys_form_and_a_second_copy_changes_nothing() {
    for (index, source) in FIXTURES.iter().enumerate() {
        let arky_form = arkys_form(source, FrontmatterFormat::Yaml);
        let json_form = arkys_form_through_json(source);
        // The picture `obsidian_note.md` embeds.
        let system = vault(source)
            .with_file("/vault/tree-sketch.png", PNG)
            .unwrap();
        for form in FORMS.iter().copied() {
            let expected = if form == "json" {
                &json_form
            } else {
                &arky_form
            };
            let first = format!("{form}/{index}.md");
            let second = format!("{form}/again/{index}.md");
            let got = arky(&system, &["get", "a.md", "-f", form], b"");
            assert_eq!(got.code, ExitCode::SUCCESS, "{form}: {}", got.stderr);
            let written = arky(&system, &["write", &first, "-f", form], &got.stdout);
            assert_eq!(
                written.code,
                ExitCode::SUCCESS,
                "{form}: {}",
                written.stderr
            );
            assert_eq!(written.stdout, b"");
            assert_eq!(written.stderr, "");
            let copied = read(&system, &format!("/vault/{first}"));
            assert_eq!(&copied, expected, "{form}: {source}");
            let again = arky(&system, &["get", &first, "-f", form], b"");
            let rewritten = arky(&system, &["write", &second, "-f", form], &again.stdout);
            assert_eq!(
                rewritten.code,
                ExitCode::SUCCESS,
                "{form}: {}",
                rewritten.stderr
            );
            assert_eq!(read(&system, &format!("/vault/{second}")), copied);
        }
    }
}

#[test]
fn a_get_with_toml_frontmatter_of_a_yaml_file_writes_toml_and_yaml_when_left_out() {
    assert!(JEKYLL_POST.starts_with("---\n"));
    let system = vault(JEKYLL_POST);
    let toml = arky(&system, &["get", "a.md", "--frontmatter", "toml"], b"");
    assert_eq!(toml.code, ExitCode::SUCCESS, "{}", toml.stderr);
    assert!(toml.stdout.starts_with(b"+++\n"));
    let toml_form = arkys_form(JEKYLL_POST, FrontmatterFormat::Toml);
    assert_eq!(toml.stdout, toml_form.as_bytes());
    assert!(ZOLA_PAGE.starts_with("+++\n"));
    let zola = vault(ZOLA_PAGE);
    let yaml = arky(&zola, &["get", "a.md"], b"");
    assert!(yaml.stdout.starts_with(b"---\n"));
    let yaml_form = arkys_form(ZOLA_PAGE, FrontmatterFormat::Yaml);
    assert_eq!(yaml.stdout, yaml_form.as_bytes());
}

#[test]
fn a_write_with_toml_frontmatter_stores_toml_and_yaml_when_left_out() {
    let system = MemorySystem::new();
    let toml = arky(
        &system,
        &["write", "/vault/toml.md", "--frontmatter", "toml"],
        JEKYLL_POST.as_bytes(),
    );
    assert_eq!(toml.code, ExitCode::SUCCESS, "{}", toml.stderr);
    let toml_file = read(&system, "/vault/toml.md");
    assert!(toml_file.starts_with("+++\n"));
    assert_eq!(toml_file, arkys_form(JEKYLL_POST, FrontmatterFormat::Toml));
    let yaml = arky(&system, &["write", "/vault/yaml.md"], ZOLA_PAGE.as_bytes());
    assert_eq!(yaml.code, ExitCode::SUCCESS, "{}", yaml.stderr);
    let yaml_file = read(&system, "/vault/yaml.md");
    assert!(yaml_file.starts_with("---\n"));
}

#[test]
fn output_and_input_name_files_of_the_machine() {
    let source = FIXTURES[7];
    let system = vault(source);
    let streamed = arky(&system, &["get", "a.md", "-f", "json"], b"");
    let got = arky(
        &system,
        &["get", "a.md", "-f", "json", "-o", "out/a.json"],
        b"",
    );
    assert_eq!(got.code, ExitCode::SUCCESS, "{}", got.stderr);
    assert_eq!(got.stdout, b"");
    assert_eq!(got.stderr, "");
    assert_eq!(
        read(&system, "/vault/out/a.json").as_bytes(),
        streamed.stdout
    );
    let written = arky(
        &system,
        &["write", "b.md", "-f", "json", "-i", "out/a.json"],
        b"",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    assert_eq!(
        read(&system, "/vault/b.md"),
        arkys_form_through_json(source)
    );
}

#[test]
fn a_failing_get_with_output_leaves_no_file_and_no_partial_file() {
    let system = vault("# A\n");
    let ran = arky(&system, &["get", "missing.md", "-o", "out.json"], b"");
    failed(&ran, 6, "arky: missing.md: no file at this path");
    let folder = system.read_dir(Path::new("/vault")).unwrap();
    assert_eq!(folder, [PathBuf::from("/vault/a.md")]);
}

#[test]
fn a_missing_path_exits_6_with_one_line_naming_the_path() {
    let system = vault("# A\n");
    let ran = arky(&system, &["get", "missing.md"], b"");
    failed(&ran, 6, "arky: missing.md: no file at this path");
    assert_eq!(ran.stderr, "arky: missing.md: no file at this path\n");
    let input = arky(&system, &["write", "b.md", "-i", "missing.json"], b"");
    failed(&input, 6, "arky: missing.json: no file at this path");
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[test]
fn text_that_is_not_json_exits_1_naming_the_path_and_stores_nothing() {
    let system = vault("# A\n");
    let ran = arky(&system, &["write", "b.md", "-f", "json"], b"nope\n");
    failed(&ran, 1, "arky: b.md: ");
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[test]
fn bytes_that_are_not_utf8_exit_1_naming_the_path_and_the_offset() {
    let system = MemorySystem::new().with_file("/a.md", b"a\xffb").unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 1, "arky: a.md: not UTF-8 at byte 1");
}

#[test]
fn a_path_that_names_no_file_exits_2() {
    let system = vault("# A\n");
    let ran = arky(&system, &["get", "../.."], b"");
    failed(&ran, 2, "arky: ../..: ");
}

#[cfg(not(feature = "minio"))]
#[test]
fn a_minio_store_exits_3_in_a_build_without_it() {
    let system = vault("# A\n");
    let minio = arky(
        &system,
        &["write", "b.md", "-s", "minio:notes/drafts"],
        b"# B\n",
    );
    failed(
        &minio,
        3,
        "arky: minio:notes/drafts: this arky is built without the MinIO store",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "minio")]
#[test]
fn each_minio_variable_left_out_exits_3_with_one_line_naming_it() {
    let lines = [
        "arky: ARKY_MINIO_ENDPOINT is not set: it holds the MinIO server's address\n",
        "arky: ARKY_MINIO_REGION is not set: it holds the region the MinIO server expects\n",
        "arky: AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key\n",
        "arky: AWS_SECRET_ACCESS_KEY is not set: it holds the MinIO secret key\n",
    ];
    for ((name, _), line) in MINIO_VARIABLES.iter().zip(lines) {
        let system = minio_variables(vault("# A\n"), Some(name));
        let got = arky(&system, &["get", "a.md", "-s", "minio:notes/vault"], b"");
        failed(&got, 3, line);
        hides_the_keys(&got);
        let written = arky(
            &system,
            &["write", "b.md", "--store", "minio:notes/vault"],
            b"# B\n",
        );
        failed(&written, 3, line);
        hides_the_keys(&written);
        assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
    }
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_server_that_cannot_be_reached_exits_4_naming_the_path_and_no_key() {
    let system = minio_variables(vault("# A\n"), None);
    let got = arky(&system, &["get", "a.md", "-s", "minio:notes"], b"");
    failed(&got, 4, "arky: a.md: ");
    hides_the_keys(&got);
    let written = arky(
        &system,
        &["write", "b.md", "-s", "minio:notes/vault"],
        b"# B\n",
    );
    failed(&written, 4, "arky: b.md: ");
    hides_the_keys(&written);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "minio")]
#[test]
fn with_a_minio_store_the_path_is_used_as_written_and_checked_before_any_request() {
    let system = minio_variables(vault("# A\n"), None);
    for path in ["/notes/a.md", "./a.md", "notes/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", "minio:notes/vault"], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path\n"),
        );
    }
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_prefix_that_is_not_a_store_path_exits_2_naming_the_store() {
    let system = vault("# A\n");
    for prefix in ["a//b", "vault/", "..", "a/./b"] {
        let store = format!("minio:notes/{prefix}");
        let ran = arky(&system, &["get", "a.md", "-s", &store], b"");
        failed(
            &ran,
            2,
            &format!("arky: {store}: `{prefix}` is not a store path\n"),
        );
    }
}

#[cfg(not(feature = "mongodb"))]
#[test]
fn a_mongodb_store_exits_3_in_a_build_without_it() {
    let system = vault("# A\n");
    let mongodb = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
    failed(
        &mongodb,
        3,
        "arky: mongodb:notes/files: this arky is built without the MongoDB store",
    );
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_store_with_no_connection_string_exits_3_with_one_line_naming_the_variable() {
    let system = vault("# A\n");
    let line = "arky: ARKY_MONGODB_URI is not set: it holds the MongoDB connection string\n";
    let got = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
    failed(&got, 3, line);
    let written = arky(
        &system,
        &["write", "b.md", "--store", "mongodb:notes/files"],
        b"# B\n",
    );
    failed(&written, 3, line);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_store_with_no_bucket_exits_3_with_one_line_naming_the_variable() {
    let system = vault("# A\n")
        .with_env("ARKY_MONGODB_URI", UNREACHABLE)
        .unwrap();
    let line = "arky: ARKY_MONGODB_BUCKET is not set: it holds the MinIO bucket of a MongoDB store's files\n";
    let got = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
    failed(&got, 3, line);
    let written = arky(
        &system,
        &["write", "b.md", "--store", "mongodb:notes/files"],
        b"# B\n",
    );
    failed(&written, 3, line);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_bucket_that_is_not_a_bucket_and_a_prefix_exits_3_naming_the_variable() {
    let shape = "arky: ARKY_MONGODB_BUCKET is not <bucket> or <bucket>/<prefix>: it holds the MinIO bucket of a MongoDB store's files\n";
    let prefix = "arky: ARKY_MONGODB_BUCKET: `a//b` is not a store path\n";
    for (bucket, line) in [
        ("", shape),
        ("/files", shape),
        ("arky-files/", shape),
        ("arky-files/a//b", prefix),
    ] {
        let system = mongodb_variables(vault("# A\n"), None)
            .with_env("ARKY_MONGODB_BUCKET", bucket)
            .unwrap();
        let got = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
        failed(&got, 3, line);
    }
}

#[cfg(feature = "mongodb")]
#[test]
fn each_minio_variable_left_out_of_a_mongodb_store_exits_3_with_one_line_naming_it() {
    let lines = [
        "arky: ARKY_MINIO_ENDPOINT is not set: it holds the MinIO server's address\n",
        "arky: ARKY_MINIO_REGION is not set: it holds the region the MinIO server expects\n",
        "arky: AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key\n",
        "arky: AWS_SECRET_ACCESS_KEY is not set: it holds the MinIO secret key\n",
    ];
    for ((name, _), line) in MINIO_VARIABLES.iter().zip(lines) {
        let system = mongodb_variables(vault("# A\n"), Some(name));
        let got = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
        failed(&got, 3, line);
        hides_the_keys(&got);
        let written = arky(
            &system,
            &["write", "b.md", "--store", "mongodb:notes/files"],
            b"# B\n",
        );
        failed(&written, 3, line);
        hides_the_keys(&written);
        assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
    }
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_server_that_cannot_be_reached_exits_4_naming_the_store() {
    let system = mongodb_variables(vault("# A\n"), None);
    let got = arky(&system, &["get", "a.md", "-s", "mongodb:notes/files"], b"");
    failed(&got, 4, "arky: mongodb:notes/files: ");
    let written = arky(
        &system,
        &["write", "b.md", "-s", "mongodb:notes/files"],
        b"# B\n",
    );
    failed(&written, 4, "arky: mongodb:notes/files: ");
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "mongodb")]
#[test]
fn with_a_mongodb_store_the_path_is_used_as_written_and_checked_before_connecting() {
    let system = mongodb_variables(vault("# A\n"), None);
    for path in ["/notes/a.md", "./a.md", "notes/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", "mongodb:notes/files"], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path"),
        );
    }
}

#[test]
fn asset_finish_deletions_of_a_minio_store_exits_2_naming_the_store() {
    let system = vault("# A\n");
    let ran = arky(
        &system,
        &["asset", "finish-deletions", "--store", "minio:arky"],
        b"",
    );
    failed(
        &ran,
        2,
        "arky: minio:arky keeps no deletions: asset finish-deletions takes --store mongodb:<database>/<collection>\n",
    );
}

#[cfg(feature = "mongodb")]
#[test]
fn asset_finish_deletions_with_no_connection_string_exits_3_with_one_line_naming_the_variable() {
    let system = minio_variables(vault("# A\n"), None)
        .with_env("ARKY_MONGODB_BUCKET", "arky-files")
        .unwrap();
    let ran = arky(
        &system,
        &[
            "asset",
            "finish-deletions",
            "--store",
            "mongodb:notes/files",
        ],
        b"",
    );
    failed(
        &ran,
        3,
        "arky: ARKY_MONGODB_URI is not set: it holds the MongoDB connection string\n",
    );
}

#[cfg(not(feature = "mongodb"))]
#[test]
fn asset_finish_deletions_exits_3_in_a_build_without_the_mongodb_store() {
    let system = vault("# A\n");
    let ran = arky(
        &system,
        &[
            "asset",
            "finish-deletions",
            "--store",
            "mongodb:notes/files",
        ],
        b"",
    );
    failed(
        &ran,
        3,
        "arky: mongodb:notes/files: this arky is built without the MongoDB store\n",
    );
}

#[test]
fn a_standard_output_that_fails_exits_4() {
    let system = vault("# A\n");
    let cli = Cli::try_parse_from(["arky", "get", "a.md"]).unwrap();
    let mut full: &mut [u8] = &mut [];
    let mut stderr = Vec::new();
    let runtime = Builder::new_current_thread().build().unwrap();
    let code = runtime.block_on(run(&cli, &system, &mut &b""[..], &mut full, &mut stderr));
    assert_eq!(code, ExitCode::from(4));
    let line = String::from_utf8(stderr).unwrap();
    assert!(line.starts_with("arky: standard output: "), "{line}");
    assert_eq!(line.lines().count(), 1, "{line}");
}

#[test]
fn a_relative_path_from_a_subfolder_names_the_file_the_shell_names() {
    let source = FIXTURES[17];
    let system = MemorySystem::new()
        .with_file("/vault/x/a.md", source.as_bytes())
        .unwrap()
        .with_dir("/vault/y")
        .unwrap()
        .with_current_dir("/vault/y")
        .unwrap();
    let expected = arkys_form(source, FrontmatterFormat::Yaml);
    for path in [
        "../x/a.md",
        "./../x/./a.md",
        "/vault/x/a.md",
        "../../vault/x/a.md",
    ] {
        let ran = arky(&system, &["get", path], b"");
        assert_eq!(ran.code, ExitCode::SUCCESS, "{path}: {}", ran.stderr);
        assert_eq!(ran.stdout, expected.as_bytes(), "{path}");
    }
    let written = arky(&system, &["write", "../x/b.md"], source.as_bytes());
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    assert_eq!(read(&system, "/vault/x/b.md"), expected);
}

#[test]
fn a_write_replaces_the_file_at_the_path() {
    let system = vault("# Old\n");
    let ran = arky(&system, &["write", "a.md"], b"# New\n");
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.stderr);
    assert_eq!(read(&system, "/vault/a.md"), "# New\n");
}

#[test]
fn a_markdown_input_brings_each_file_it_embeds_and_a_markdown_output_writes_each_beside_it() {
    let system = MemorySystem::new()
        .with_file("/vault/draft/report.md", REPORT.as_bytes())
        .unwrap()
        .with_file("/vault/draft/media/logo.png", PNG)
        .unwrap()
        .with_current_dir("/vault")
        .unwrap();
    let written = arky(
        &system,
        &["write", "notes/report.md", "-i", "draft/report.md"],
        b"",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    reported(&system, "/vault/notes/report.md");
    let got = arky(
        &system,
        &["get", "notes/report.md", "-o", "out/report.md"],
        b"",
    );
    assert_eq!(got.code, ExitCode::SUCCESS, "{}", got.stderr);
    assert_eq!(got.stdout, b"");
    assert_eq!(got.stderr, "");
    reported(&system, "/vault/out/report.md");
}

#[test]
fn an_embedded_output_is_one_file_holding_each_embed_as_a_named_data_uri() {
    let system = report();
    let got = arky(
        &system,
        &["get", "notes/report.md", "-o", "out/one.md", "--embed"],
        b"",
    );
    assert_eq!(got.code, ExitCode::SUCCESS, "{}", got.stderr);
    assert_eq!(got.stdout, b"");
    let one = read(&system, "/vault/out/one.md");
    assert!(
        one.contains("![Logo](data:image/png;name=media/logo.png;base64,"),
        "{one}"
    );
    let folder = system.read_dir(Path::new("/vault/out")).unwrap();
    assert_eq!(folder, [PathBuf::from("/vault/out/one.md")]);
    let streamed = arky(&system, &["get", "notes/report.md"], b"");
    assert_eq!(streamed.stdout, one.as_bytes());
}

#[test]
fn each_form_through_standard_output_carries_each_file_to_the_copy() {
    let system = report();
    for form in FORMS.iter().copied() {
        let got = arky(&system, &["get", "notes/report.md", "-f", form], b"");
        assert_eq!(got.code, ExitCode::SUCCESS, "{form}: {}", got.stderr);
        let copy = format!("{form}/report.md");
        let written = arky(&system, &["write", &copy, "-f", form], &got.stdout);
        assert_eq!(
            written.code,
            ExitCode::SUCCESS,
            "{form}: {}",
            written.stderr
        );
        reported(&system, &format!("/vault/{copy}"));
    }
    let markdown = arky(&system, &["get", "notes/report.md"], b"");
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(
        text.contains("![Logo](data:image/png;name=media/logo.png;base64,"),
        "{text}"
    );
}

#[test]
fn binary_json_and_zip_to_an_output_are_bytes_a_write_from_that_input_reads_with_each_file() {
    let system = report();
    for form in [
        #[cfg(feature = "binary-format")]
        "binary",
        "json",
        "zip",
    ] {
        let output = format!("out/report.{form}");
        let got = arky(
            &system,
            &["get", "notes/report.md", "-f", form, "-o", &output],
            b"",
        );
        assert_eq!(got.code, ExitCode::SUCCESS, "{form}: {}", got.stderr);
        assert_eq!(got.stdout, b"");
        let copy = format!("{form}/report.md");
        let written = arky(&system, &["write", &copy, "-f", form, "-i", &output], b"");
        assert_eq!(
            written.code,
            ExitCode::SUCCESS,
            "{form}: {}",
            written.stderr
        );
        reported(&system, &format!("/vault/{copy}"));
    }
    assert!(!system.exists(Path::new("/vault/out/media")).unwrap());
    let forms: &[&str] = &[
        #[cfg(feature = "binary-format")]
        "binary",
        "json",
    ];
    for form in forms.iter().copied() {
        let streamed = arky(&system, &["get", "notes/report.md", "-f", form], b"");
        let output = format!("/vault/out/report.{form}");
        assert_eq!(bytes_of(&system, &output), streamed.stdout, "{form}");
    }
}

#[test]
fn a_zip_holds_index_md_and_each_file_at_its_path() {
    let system = report();
    let got = arky(
        &system,
        &["get", "notes/report.md", "-f", "zip", "-o", "r.zip"],
        b"",
    );
    assert_eq!(got.code, ExitCode::SUCCESS, "{}", got.stderr);
    let zip = bytes_of(&system, "/vault/r.zip");
    let bundle = decode(&zip, Form::Zip, Missing::Refuse).unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    assert_eq!(bundle.file("media/logo.png").unwrap(), PNG);
    let written = arky(
        &system,
        &["write", "z/report.md", "-f", "zip", "-i", "r.zip"],
        b"",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    reported(&system, "/vault/z/report.md");
}

#[test]
fn a_missing_embedded_file_fails_get_and_write_with_code_1_and_allow_missing_keeps_its_embed() {
    let line = "arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it\n";
    let text = b"![Logo](media/none.png)\n";
    let system = vault("# A\n");
    let refused = arky(&system, &["write", "notes/a.md"], text);
    failed(&refused, 1, line);
    assert!(!system.exists(Path::new("/vault/notes/a.md")).unwrap());
    let stored = arky(&system, &["write", "notes/a.md", "--allow-missing"], text);
    assert_eq!(stored.code, ExitCode::SUCCESS, "{}", stored.stderr);
    assert_eq!(read(&system, "/vault/notes/a.md").as_bytes(), text);
    for arguments in [
        ["get", "notes/a.md"].as_slice(),
        ["get", "notes/a.md", "-f", "zip"].as_slice(),
        ["get", "notes/a.md", "-o", "out/a.md"].as_slice(),
    ] {
        failed(&arky(&system, arguments, b""), 1, line);
    }
    assert!(!system.exists(Path::new("/vault/out")).unwrap());
    let given = arky(&system, &["get", "notes/a.md", "--allow-missing"], b"");
    assert_eq!(given.code, ExitCode::SUCCESS, "{}", given.stderr);
    assert_eq!(given.stdout, text);
    let output = ["get", "notes/a.md", "-o", "out/a.md", "--allow-missing"];
    let beside = arky(&system, &output, b"");
    assert_eq!(beside.code, ExitCode::SUCCESS, "{}", beside.stderr);
    let folder = system.read_dir(Path::new("/vault/out")).unwrap();
    assert_eq!(folder, [PathBuf::from("/vault/out/a.md")]);
    assert_eq!(read(&system, "/vault/out/a.md").as_bytes(), text);
}

#[test]
fn a_markdown_input_whose_embedded_file_is_missing_fails_naming_the_input_unless_allowed() {
    let system = vault("![Logo](media/none.png)\n");
    let refused = arky(&system, &["write", "b.md", "-i", "a.md"], b"");
    failed(
        &refused,
        1,
        "arky: a.md: index.md names media/none.png, and the bundle does not hold it\n",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
    let stored = arky(
        &system,
        &["write", "b.md", "-i", "a.md", "--allow-missing"],
        b"",
    );
    assert_eq!(stored.code, ExitCode::SUCCESS, "{}", stored.stderr);
    assert_eq!(read(&system, "/vault/b.md"), "![Logo](media/none.png)\n");
}

#[test]
fn an_embed_that_leaves_the_folder_fails_with_code_1_and_allow_missing_keeps_it() {
    let text = b"![Logo](../logo.png)\n";
    let system = vault("# A\n").with_file("/vault/logo.png", PNG).unwrap();
    let refused = arky(&system, &["write", "notes/a.md"], text);
    failed(
        &refused,
        1,
        "arky: notes/a.md: index.md names ../logo.png, which leaves the bundle's folder\n",
    );
    let stored = arky(&system, &["write", "notes/a.md", "--allow-missing"], text);
    assert_eq!(stored.code, ExitCode::SUCCESS, "{}", stored.stderr);
    assert_eq!(read(&system, "/vault/notes/a.md").as_bytes(), text);
    let got = arky(&system, &["get", "notes/a.md"], b"");
    failed(
        &got,
        1,
        "arky: notes/a.md: index.md names ../logo.png, which leaves the bundle's folder\n",
    );
    let given = arky(&system, &["get", "notes/a.md", "--allow-missing"], b"");
    assert_eq!(given.code, ExitCode::SUCCESS, "{}", given.stderr);
    assert_eq!(given.stdout, text);
}

#[test]
fn embed_with_a_form_other_than_markdown_exits_2_and_writes_nothing() {
    let system = report();
    for form in [
        #[cfg(feature = "binary-format")]
        "binary",
        "json",
        "zip",
    ] {
        let output = format!("r.{form}");
        let ran = arky(
            &system,
            &[
                "get",
                "notes/report.md",
                "-f",
                form,
                "-o",
                &output,
                "--embed",
            ],
            b"",
        );
        failed(
            &ran,
            2,
            "arky: --embed writes markdown; it needs -f markdown\n",
        );
        assert!(!system.exists(&Path::new("/vault").join(&output)).unwrap());
    }
}

#[test]
fn with_no_configuration_file_a_save_waits_2_seconds_on_a_lock_that_has_not_expired() {
    let system = locked(0, &[]);
    let ran = arky(&system, &SAVE, b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(2));
}

#[test]
fn a_configuration_file_with_no_lock_table_gives_the_defaults() {
    let system = locked(0, &[("/vault/arky.toml", "[vault]\nroot = \".\"\n")]);
    let ran = arky(&system, &SAVE, b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(2));
}

#[test]
fn wait_seconds_sets_how_long_a_save_waits_on_a_lock_that_has_not_expired() {
    let system = locked(
        0,
        &[(
            "/vault/arky.toml",
            "[lock]\nexpires_seconds = 60\nwait_seconds = 7\n",
        )],
    );
    let ran = arky(&system, &SAVE, b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(7));
}

#[test]
fn expires_seconds_sets_when_a_lock_of_another_save_may_be_removed() {
    let config = ("/vault/arky.toml", "[lock]\nexpires_seconds = 1\n");
    let system = locked(2, &[config]);
    let ran = arky(&system, &SAVE, b"");
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.stderr);
    assert!(!system.exists(Path::new(LOCK)).unwrap());
    reported(&system, "/vault/notes/report.md");
    assert_eq!(since_start(&system), Duration::ZERO);
}

#[test]
fn a_lock_of_another_save_inside_the_default_expiry_is_kept_by_a_longer_expiry_than_its_age() {
    let config = ("/vault/arky.toml", "[lock]\nexpires_seconds = 10\n");
    let system = locked(6, &[config]);
    let ran = arky(&system, &SAVE, b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn config_is_accepted_before_and_after_the_command_name() {
    let system = locked(
        0,
        &[(
            "/vault/other.toml",
            "[lock]\nexpires_seconds = 60\nwait_seconds = 3\n",
        )],
    );
    let after = arky(
        &system,
        &[&SAVE[..], &["--config", "other.toml"]].concat(),
        b"",
    );
    failed(&after, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(3));
    let before = arky(&system, &[&["-c", "other.toml"][..], &SAVE].concat(), b"");
    failed(&before, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(6));
}

#[test]
fn a_configuration_file_that_config_names_and_is_missing_exits_3_before_the_command_runs() {
    let system = vault("# A\n");
    let ran = arky(&system, &["get", "a.md", "--config", "other.toml"], b"");
    failed(
        &ran,
        3,
        "arky: other.toml: the configuration file does not exist\n",
    );
    let written = arky(&system, &["-c", "other.toml", "write", "b.md"], b"# B\n");
    failed(
        &written,
        3,
        "arky: other.toml: the configuration file does not exist\n",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[test]
fn arky_config_names_the_file_and_a_missing_one_exits_3() {
    let system = locked(0, &[("/vault/b.toml", "[lock]\nwait_seconds = 4\n")])
        .with_env("ARKY_CONFIG", "b.toml")
        .unwrap();
    let ran = arky(&system, &SAVE, b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(4));
    let missing = vault("# A\n").with_env("ARKY_CONFIG", "gone.toml").unwrap();
    let got = arky(&missing, &["get", "a.md"], b"");
    failed(
        &got,
        3,
        "arky: gone.toml: the configuration file does not exist\n",
    );
}

#[test]
fn config_wins_over_arky_config() {
    let files = [
        (
            "/vault/a.toml",
            "[lock]\nexpires_seconds = 60\nwait_seconds = 3\n",
        ),
        ("/vault/b.toml", "[lock]\nwait_seconds = 9\n"),
    ];
    let system = locked(0, &files).with_env("ARKY_CONFIG", "b.toml").unwrap();
    let ran = arky(&system, &[&SAVE[..], &["--config", "a.toml"]].concat(), b"");
    failed(&ran, 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(3));
}

#[test]
fn a_value_that_is_not_a_whole_number_above_zero_exits_3_naming_the_key() {
    for (key, value) in [
        ("expires_seconds", "0"),
        ("expires_seconds", "-1"),
        ("wait_seconds", "\"5\""),
        ("wait_seconds", "2.5"),
        ("wait_seconds", "true"),
    ] {
        let config = format!("[lock]\n{key} = {value}\n");
        let system = vault("# A\n")
            .with_file("/vault/arky.toml", config.as_bytes())
            .unwrap();
        let ran = arky(&system, &["get", "a.md"], b"");
        let line = format!(
            "arky: arky.toml: {key} in [lock] is not a whole number of seconds above zero\n"
        );
        failed(&ran, 3, &line);
    }
}

#[test]
fn a_key_of_the_lock_table_other_than_the_two_exits_3_naming_it() {
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", b"[lock]\nexpire_seconds = 5\n")
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(
        &ran,
        3,
        "arky: arky.toml: [lock] has no key expire_seconds; it takes expires_seconds and wait_seconds\n",
    );
}

#[test]
fn a_lock_that_is_not_a_table_exits_3() {
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", b"lock = 5\n")
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 3, "arky: arky.toml: lock is not a table\n");
}

#[test]
fn a_configuration_file_the_reader_cannot_read_exits_3_with_the_readers_error() {
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", b"[lock\n")
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 3, "arky: arky.toml: ");
    let bad = vault("# A\n")
        .with_file("/vault/arky.toml", &[0xff, 0xfe])
        .unwrap();
    let unread = arky(&bad, &["get", "a.md"], b"");
    failed(&unread, 3, "arky: arky.toml: ");
}

#[test]
fn the_other_tables_of_the_configuration_file_are_left_alone() {
    let config = "[vault]\nroot = \".\"\n\n[rules]\nstrict = true\n\n[lock]\nexpires_seconds = 5 # five\n\n[ai]\nmodel = \"x\"\n";
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", config.as_bytes())
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.stderr);
}

/// A system whose current folder is `/vault`, holding each of `files` as it
/// was `ago` before `START`, and a clock at `START`.
fn aged(ago: u64, files: &[(&str, &str)]) -> MemorySystem {
    let system = files.iter().fold(
        MemorySystem::new()
            .with_now(started() - Duration::from_secs(ago))
            .unwrap()
            .with_current_dir("/vault")
            .unwrap(),
        |with, (path, text)| with.with_file(path, text.as_bytes()).unwrap(),
    );
    system.advance(Duration::from_secs(ago)).unwrap();
    system
}

/// Asserts `ran` succeeded and wrote exactly `lines` to standard output.
fn wrote_lines(ran: &Ran, lines: &str) {
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.stderr);
    assert_eq!(String::from_utf8_lossy(&ran.stdout), lines);
    assert_eq!(ran.stderr, "");
}

#[test]
fn fix_removes_an_expired_lock_and_writes_its_path_as_the_folder_was_named() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&ran, "notes/report.md.arky-lock\n");
    assert!(!system.exists(Path::new(LOCK)).unwrap());
    let again = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&again, "");
}

#[test]
fn fix_writes_an_absolute_folder_as_given() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "/vault/notes"], b"");
    wrote_lines(&ran, "/vault/notes/report.md.arky-lock\n");
}

#[test]
fn fix_leaves_a_lock_that_has_not_expired() {
    let system = locked(0, &[]);
    let ran = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&ran, "");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_writes_the_locks_of_nested_folders_sorted() {
    let system = aged(
        6,
        &[
            ("/vault/notes/report.md.arky-lock", "another"),
            ("/vault/notes/parts/b.md.arky-lock", "another"),
            ("/vault/notes/a.md.arky-lock", "another"),
        ],
    );
    let ran = arky(&system, &["fix", "notes"], b"");
    wrote_lines(
        &ran,
        "notes/a.md.arky-lock\nnotes/parts/b.md.arky-lock\nnotes/report.md.arky-lock\n",
    );
}

#[test]
fn fix_with_no_path_works_on_the_current_folder_and_writes_paths_below_it() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix"], b"");
    wrote_lines(&ran, "notes/report.md.arky-lock\n");
    let nested = locked(6, &[]).with_current_dir("/vault/notes").unwrap();
    let from_notes = arky(&nested, &["fix"], b"");
    wrote_lines(&from_notes, "report.md.arky-lock\n");
}

#[test]
fn fix_of_the_root_takes_the_whole_store_and_writes_each_path_from_the_root() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "/"], b"");
    wrote_lines(&ran, "/vault/notes/report.md.arky-lock\n");
    assert!(!system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_with_no_path_run_in_the_root_writes_each_path_below_the_root() {
    let system = locked(6, &[]).with_current_dir("/").unwrap();
    let ran = arky(&system, &["fix"], b"");
    wrote_lines(&ran, "vault/notes/report.md.arky-lock\n");
    assert!(!system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn get_and_write_of_the_root_exit_2() {
    let system = locked(6, &[]);
    for command in [["get", "/"], ["write", "/"]] {
        let ran = arky(&system, &command, b"");
        assert_eq!(ran.code, ExitCode::from(2), "{}", ran.stderr);
    }
}

#[test]
fn fix_leaves_the_lock_files_of_other_tools() {
    let system = aged(
        60,
        &[
            ("/vault/Cargo.lock", "old"),
            ("/vault/notes/yarn.lock", "old"),
        ],
    );
    let ran = arky(&system, &["fix"], b"");
    wrote_lines(&ran, "");
    assert_eq!(bytes_of(&system, "/vault/Cargo.lock"), b"old");
    assert_eq!(bytes_of(&system, "/vault/notes/yarn.lock"), b"old");
}

#[test]
fn fix_of_a_file_exits_2_naming_the_path() {
    let system = locked(6, &[("/vault/notes/report.md", REPORT)]);
    let ran = arky(&system, &["fix", "notes/report.md"], b"");
    failed(&ran, 2, "arky: notes/report.md: not a folder\n");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_of_a_folder_that_is_not_there_exits_2_naming_the_path() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "missing"], b"");
    failed(&ran, 2, "arky: missing: not a folder\n");
}

#[test]
fn fix_of_a_mongodb_store_exits_2_naming_the_store() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "--store", "mongodb:notes/docs"], b"");
    failed(
        &ran,
        2,
        "arky: mongodb:notes/docs keeps no locks: fix takes the file system or --store minio:<bucket>\n",
    );
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn expires_seconds_decides_which_locks_fix_removes() {
    let system = locked(
        10,
        &[("/vault/arky.toml", "[lock]\nexpires_seconds = 60\n")],
    );
    let ran = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&ran, "");
    assert!(system.exists(Path::new(LOCK)).unwrap());
    system.advance(Duration::from_secs(60)).unwrap();
    let later = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&later, "notes/report.md.arky-lock\n");
}

#[test]
fn fix_with_a_bad_configuration_file_exits_3_before_touching_a_lock() {
    let system = locked(6, &[("/vault/arky.toml", "[lock]\nexpires_seconds = 0\n")]);
    let ran = arky(&system, &["fix", "notes"], b"");
    failed(&ran, 3, "arky: arky.toml: ");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[cfg(not(feature = "minio"))]
#[test]
fn fix_of_a_minio_store_exits_3_in_a_build_without_it() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "-s", "minio:notes/drafts"], b"");
    failed(
        &ran,
        3,
        "arky: minio:notes/drafts: this arky is built without the MinIO store\n",
    );
}

#[cfg(feature = "minio")]
#[test]
fn fix_of_a_minio_server_that_cannot_be_reached_exits_4_naming_the_store_and_no_key() {
    let system = minio_variables(locked(6, &[]), None);
    let ran = arky(&system, &["fix", "-s", "minio:notes"], b"");
    failed(&ran, 4, "arky: minio:notes: ");
    hides_the_keys(&ran);
}

#[cfg(feature = "minio")]
#[test]
fn fix_of_a_minio_store_with_a_variable_left_out_exits_3_naming_it() {
    let system = minio_variables(locked(6, &[]), Some("ARKY_MINIO_ENDPOINT"));
    let ran = arky(&system, &["fix", "-s", "minio:notes"], b"");
    failed(
        &ran,
        3,
        "arky: ARKY_MINIO_ENDPOINT is not set: it holds the MinIO server's address\n",
    );
}

#[cfg(feature = "minio")]
#[test]
fn fix_of_a_minio_path_that_is_not_a_store_path_exits_2_before_any_request() {
    let system = minio_variables(locked(6, &[]), None);
    let ran = arky(&system, &["fix", "./notes", "-s", "minio:notes"], b"");
    failed(&ran, 2, "arky: ./notes: `./notes` is not a store path\n");
}

#[test]
fn a_standard_output_that_fails_exits_4_after_fix_removed_a_lock() {
    let system = locked(6, &[]);
    let cli = Cli::try_parse_from(["arky", "fix", "notes"]).unwrap();
    let mut full: &mut [u8] = &mut [];
    let mut stderr = Vec::new();
    let runtime = Builder::new_current_thread().build().unwrap();
    let code = runtime.block_on(run(&cli, &system, &mut &b""[..], &mut full, &mut stderr));
    assert_eq!(code, ExitCode::from(4));
    let line = String::from_utf8(stderr).unwrap();
    assert!(line.starts_with("arky: standard output: "), "{line}");
}
