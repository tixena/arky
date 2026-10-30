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

/// The values a store string's keys come from: `${KEY}` and `${SECRET}` in
/// the string, else the AWS keys. No output may show them.
#[cfg(any(feature = "minio", feature = "mongodb", feature = "s3"))]
const KEYS: [(&str, &str); 4] = [
    ("AWS_ACCESS_KEY_ID", "arky-unit-access"),
    ("AWS_SECRET_ACCESS_KEY", "arky-unit-secret"),
    ("KEY", "arky-unit-key"),
    ("SECRET", "arky-unit-secret-key"),
];

const LOCK: &str = "/vault/notes/report.md.arky-lock";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

const SAVE: [&str; 4] = ["write", "notes/report.md", "-i", "draft/report.md"];

const START: u64 = 1_792_700_000;

/// A `MinIO` server no process listens on, whose connection is refused at
/// once, over the keys `${KEY}` and `${SECRET}`.
#[cfg(feature = "minio")]
const UNREACHABLE_MINIO: &str = "minio+http://${KEY}:${SECRET}@127.0.0.1:1/notes";

/// A MongoDB server no process listens on, which the driver tries once and
/// gives up on at once, its files in a bucket of `UNREACHABLE_MINIO`'s server.
#[cfg(feature = "mongodb")]
const UNREACHABLE_MONGODB: &str = "mongodb://127.0.0.1:1/notes/files?loadBalanced=true&retryReads=false&files=minio%2Bhttp%3A%2F%2F127.0.0.1%3A1%2Farky-files";

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

/// Asserts `ran` succeeded and wrote `text` to standard output.
fn gave(ran: &Ran, text: &str) {
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.stderr);
    assert_eq!(String::from_utf8_lossy(&ran.stdout), text);
}

/// Asserts no value of `KEYS` is on `ran`'s standard error.
#[cfg(any(feature = "minio", feature = "mongodb", feature = "s3"))]
fn hides_the_keys(ran: &Ran) {
    for (_, value) in KEYS {
        assert!(!ran.stderr.contains(value), "{}", ran.stderr);
    }
}

/// `system` with the variables of `KEYS`.
#[cfg(any(feature = "minio", feature = "mongodb", feature = "s3"))]
fn keyed(system: MemorySystem) -> MemorySystem {
    KEYS.iter().fold(system, |with, (name, value)| {
        with.with_env(name, value).unwrap()
    })
}

/// A system whose current folder is `/h/vault`, holding `a.md` there, in
/// `/h/n` and in `/h/w`, each titled after its folder, and
/// `/h/w/drafts/ideas.md`. `/h/arky.toml` saves `notes` as `file:n` and
/// `work` as `file:w`, and names `default` when it is given.
fn saved(default: Option<&str>) -> MemorySystem {
    let mut toml = default.map_or_else(String::new, |name| format!("default = \"{name}\"\n"));
    toml.push_str("[stores.notes]\nurl = \"file:n\"\n\n[stores.work]\nurl = \"file:w\"\n");
    MemorySystem::new()
        .with_file("/h/arky.toml", toml.as_bytes())
        .unwrap()
        .with_file("/h/vault/a.md", b"# Vault\n")
        .unwrap()
        .with_file("/h/n/a.md", b"# Notes\n")
        .unwrap()
        .with_file("/h/w/a.md", b"# Work\n")
        .unwrap()
        .with_file("/h/w/drafts/ideas.md", b"# Ideas\n")
        .unwrap()
        .with_current_dir("/h/vault")
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

#[test]
fn with_no_store_named_the_default_store_wins_over_the_current_folder() {
    gave(
        &arky(&saved(Some("notes")), &["get", "a.md"], b""),
        "# Notes\n",
    );
    gave(&arky(&saved(None), &["get", "a.md"], b""), "# Vault\n");
    let system = saved(Some("notes"));
    let written = arky(&system, &["write", "b.md"], b"# B\n");
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    assert_eq!(read(&system, "/h/n/b.md"), "# B\n");
    assert!(!system.exists(Path::new("/h/vault/b.md")).unwrap());
}

#[test]
fn arky_store_wins_over_the_default() {
    let system = saved(Some("notes"))
        .with_env("ARKY_STORE", "arky://work")
        .unwrap();
    gave(&arky(&system, &["get", "a.md"], b""), "# Work\n");
    let folder = saved(Some("notes")).with_env("ARKY_STORE", "../w").unwrap();
    gave(&arky(&folder, &["get", "a.md"], b""), "# Work\n");
}

#[test]
fn store_wins_over_arky_store() {
    let system = saved(Some("notes"))
        .with_env("ARKY_STORE", "arky://work")
        .unwrap();
    let ran = arky(&system, &["get", "a.md", "-s", "arky://notes"], b"");
    gave(&ran, "# Notes\n");
}

#[test]
fn a_store_string_in_the_path_wins_over_store() {
    let system = saved(Some("notes"))
        .with_env("ARKY_STORE", "arky://notes")
        .unwrap();
    for path in ["arky://work/a.md", "file:///h/w/a.md", "file:/h/w/a.md"] {
        let ran = arky(&system, &["get", path, "-s", "arky://notes"], b"");
        gave(&ran, "# Work\n");
    }
    let written = arky(
        &system,
        &["write", "arky://work/b.md", "-s", "arky://notes"],
        b"# B\n",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    assert_eq!(read(&system, "/h/w/b.md"), "# B\n");
}

#[test]
fn a_relative_file_string_in_the_path_goes_up_a_folder_as_the_plain_path_does() {
    let system = MemorySystem::new()
        .with_file("/v/w/a.md", b"# W\n")
        .unwrap()
        .with_current_dir("/v/x")
        .unwrap();
    for path in ["file:../w/a.md", "../w/a.md"] {
        gave(&arky(&system, &["get", path], b""), "# W\n");
    }
}

#[test]
fn an_absolute_file_string_in_the_path_goes_up_a_folder_as_the_plain_path_does() {
    let system = MemorySystem::new()
        .with_file("/a.md", b"# A\n")
        .unwrap()
        .with_current_dir("/v")
        .unwrap();
    for path in ["file:///srv/../a.md", "/srv/../a.md"] {
        gave(&arky(&system, &["get", path], b""), "# A\n");
    }
}

#[test]
fn a_path_after_the_store_in_store_is_a_folder_inside_it() {
    let system = saved(None);
    for store in ["arky://work/drafts", "../w/drafts", "file:../w/drafts"] {
        let ran = arky(&system, &["get", "ideas.md", "-s", store], b"");
        gave(&ran, "# Ideas\n");
    }
    let written = arky(
        &system,
        &["write", "b.md", "-s", "arky://work/drafts"],
        b"# B\n",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    assert_eq!(read(&system, "/h/w/drafts/b.md"), "# B\n");
}

#[test]
fn a_path_whose_name_holds_a_colon_is_a_file_of_the_current_folder() {
    let system = saved(None)
        .with_file("/h/vault/notes:report.md", b"# Report\n")
        .unwrap();
    gave(
        &arky(&system, &["get", "notes:report.md"], b""),
        "# Report\n",
    );
}

#[test]
fn in_a_store_the_path_is_used_as_written() {
    let system = saved(None);
    for path in ["/a.md", "./a.md", "drafts/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", "arky://work"], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path\n"),
        );
    }
}

#[test]
fn each_short_form_exits_2_naming_it_and_a_folder_of_that_name_starts_with_a_dot() {
    let system = saved(None)
        .with_file("/h/vault/minio:notes/a.md", b"# Folder\n")
        .unwrap();
    let refused = [
        (
            "minio:notes",
            "minio: is not a store string: a MinIO store is minio+http://<host>/<bucket>",
        ),
        (
            "s3:notes",
            "s3: is not a store string: an S3 store is s3://<bucket>?region=<region>",
        ),
        (
            "mongodb:db/coll",
            "mongodb: is not a store string: a MongoDB store is mongodb://<host>/<database>/<collection>?files=<MinIO store>",
        ),
    ];
    for (text, why) in refused {
        let line = format!("arky: `{text}`: {why}, and a folder of this name starts with ./\n");
        failed(&arky(&system, &["get", "a.md", "-s", text], b""), 2, &line);
        failed(
            &arky(&system, &["write", "b.md", "-s", text], b"# B\n"),
            2,
            &line,
        );
        failed(&arky(&system, &["fix", "-s", text], b""), 2, &line);
        let finish = ["asset", "finish-deletions", "-s", text];
        failed(&arky(&system, &finish, b""), 2, &line);
    }
    assert!(!system.exists(Path::new("/h/vault/b.md")).unwrap());
    let variable = saved(None).with_env("ARKY_STORE", "minio:notes").unwrap();
    failed(
        &arky(&variable, &["get", "a.md"], b""),
        2,
        "arky: ARKY_STORE: minio: is not a store string: ",
    );
    gave(
        &arky(&system, &["get", "a.md", "-s", "./minio:notes"], b""),
        "# Folder\n",
    );
}

#[test]
fn an_unknown_saved_name_exits_3_naming_it() {
    let system = saved(None);
    let named = arky(&system, &["get", "a.md", "-s", "arky://nope"], b"");
    failed(
        &named,
        3,
        "arky: `arky://nope`: no store named nope in any arky.toml\n",
    );
    let in_path = arky(&system, &["get", "arky://nope/a.md"], b"");
    failed(
        &in_path,
        3,
        "arky: `arky://nope/a.md`: no store named nope in any arky.toml\n",
    );
    let written = arky(&system, &["write", "b.md", "-s", "arky://nope"], b"# B\n");
    failed(&written, 3, "arky: `arky://nope`: ");
    let default = arky(&saved(Some("gone")), &["get", "a.md"], b"");
    failed(
        &default,
        3,
        "arky: `arky://gone`: no store named gone in any arky.toml\n",
    );
}

#[test]
fn a_store_string_that_does_not_read_exits_2_and_one_that_cannot_open_exits_3() {
    let system = saved(None);
    let refused = [
        (
            ["get", "a.md", "-s", "file:n?x=1"],
            2,
            "arky: `file:n?x=1`: unknown option x\n",
        ),
        (
            ["get", "a.md", "-s", "arky://"],
            2,
            "arky: `arky://`: no store name\n",
        ),
        (
            ["get", "a.md", "-s", "hdfs://h/v"],
            3,
            "arky: `hdfs://h/v`: no backend for hdfs in this build\n",
        ),
        (
            ["get", "a.md", "-s", "${UNSET}/n"],
            3,
            "arky: UNSET is not set: `${UNSET}/n` reads it\n",
        ),
        (
            ["get", "hdfs://h/a.md", "-s", "arky://work"],
            3,
            "arky: `hdfs://h/a.md`: no backend for hdfs in this build\n",
        ),
        (
            ["write", "file:a.md?x=1", "-s", "arky://work"],
            2,
            "arky: `file:a.md?x=1`: unknown option x\n",
        ),
    ];
    for (arguments, code, line) in refused {
        failed(&arky(&system, &arguments, b"# B\n"), code, line);
    }
    assert!(!system.exists(Path::new("/h/w/b.md")).unwrap());
}

#[test]
fn an_error_of_the_store_arky_store_names_names_the_variable_and_never_its_value() {
    let value = "file:n?token=arky-unit-value";
    let system = saved(None).with_env("ARKY_STORE", value).unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 2, "arky: ARKY_STORE: unknown option token\n");
    assert!(!ran.stderr.contains("arky-unit-value"), "{}", ran.stderr);
    let unknown = saved(None).with_env("ARKY_STORE", "arky://nope").unwrap();
    failed(
        &arky(&unknown, &["write", "b.md"], b"# B\n"),
        3,
        "arky: ARKY_STORE: no store named nope in any arky.toml\n",
    );
}

#[test]
fn config_and_arky_config_read_the_one_file_they_name_and_no_other() {
    let one = "[stores.solo]\nurl = \"file:///h/w\"\n";
    let system = saved(Some("notes"))
        .with_file("/h/vault/one.toml", one.as_bytes())
        .unwrap();
    gave(&arky(&system, &["get", "a.md"], b""), "# Notes\n");
    gave(
        &arky(&system, &["get", "a.md", "-c", "one.toml"], b""),
        "# Vault\n",
    );
    let walked = arky(
        &system,
        &["get", "a.md", "-c", "one.toml", "-s", "arky://notes"],
        b"",
    );
    failed(
        &walked,
        3,
        "arky: `arky://notes`: no store named notes in any arky.toml\n",
    );
    let solo = arky(
        &system,
        &["get", "a.md", "-c", "one.toml", "-s", "arky://solo"],
        b"",
    );
    gave(&solo, "# Work\n");
    let variable = saved(Some("notes"))
        .with_file("/h/vault/one.toml", one.as_bytes())
        .unwrap()
        .with_env("ARKY_CONFIG", "one.toml")
        .unwrap();
    gave(&arky(&variable, &["get", "a.md"], b""), "# Vault\n");
}

#[test]
fn a_write_to_a_file_store_has_no_container_to_create_and_stores_the_document() {
    let system = saved(None)
        .with_file("/h/vault/draft/report.md", REPORT.as_bytes())
        .unwrap()
        .with_file("/h/vault/draft/media/logo.png", PNG)
        .unwrap();
    let written = arky(
        &system,
        &[
            "write",
            "report.md",
            "-i",
            "draft/report.md",
            "-s",
            "arky://work",
        ],
        b"",
    );
    assert_eq!(written.code, ExitCode::SUCCESS, "{}", written.stderr);
    reported(&system, "/h/w/report.md");
    let streamed = arky(&system, &["write", "b.md", "-s", "arky://work"], b"# B\n");
    assert_eq!(streamed.code, ExitCode::SUCCESS, "{}", streamed.stderr);
    assert_eq!(read(&system, "/h/w/b.md"), "# B\n");
}

#[test]
fn asset_finish_deletions_of_a_file_store_exits_2_naming_it() {
    let system = saved(None);
    for (store, line) in [
        (
            "../w",
            "arky: `../w` keeps no deletions: asset finish-deletions takes a MongoDB store\n",
        ),
        (
            "arky://work/drafts",
            "arky: `arky://work/drafts` keeps no deletions: asset finish-deletions takes a MongoDB store\n",
        ),
    ] {
        let ran = arky(
            &system,
            &["asset", "finish-deletions", "--store", store],
            b"",
        );
        failed(&ran, 2, line);
    }
}

#[cfg(not(feature = "minio"))]
#[test]
fn a_minio_store_exits_3_in_a_build_without_it() {
    let system = vault("# A\n");
    let store = "minio+http://127.0.0.1:1/notes";
    let ran = arky(&system, &["write", "b.md", "-s", store], b"# B\n");
    failed(
        &ran,
        3,
        "arky: `minio+http://127.0.0.1:1/notes`: no backend for minio+http in this build\n",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_store_with_no_keys_exits_3_naming_the_store_and_the_variable() {
    let system = vault("# A\n");
    let store = "minio+http://127.0.0.1:1/notes";
    let line = "arky: `minio+http://127.0.0.1:1/notes`: AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key\n";
    failed(&arky(&system, &["get", "a.md", "-s", store], b""), 3, line);
    failed(
        &arky(&system, &["write", "b.md", "-s", store], b"# B\n"),
        3,
        line,
    );
    let ran = arky(&system, &["get", "a.md", "-s", UNREACHABLE_MINIO], b"");
    failed(
        &ran,
        3,
        "arky: KEY is not set: `minio+http://${KEY}:${SECRET}@127.0.0.1:1/notes` reads it\n",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_server_that_cannot_be_reached_exits_4_naming_the_path_on_a_get_and_the_store_on_a_write()
{
    let system = keyed(vault("# A\n"));
    let got = arky(&system, &["get", "a.md", "-s", UNREACHABLE_MINIO], b"");
    failed(&got, 4, "arky: a.md: ");
    hides_the_keys(&got);
    let written = arky(
        &system,
        &["write", "b.md", "-s", UNREACHABLE_MINIO],
        b"# B\n",
    );
    failed(
        &written,
        4,
        "arky: `minio+http://${KEY}:${SECRET}@127.0.0.1:1/notes`: ",
    );
    hides_the_keys(&written);
    let fixed = arky(&system, &["fix", "-s", UNREACHABLE_MINIO], b"");
    failed(
        &fixed,
        4,
        "arky: `minio+http://${KEY}:${SECRET}@127.0.0.1:1/notes`: ",
    );
    hides_the_keys(&fixed);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "minio")]
#[test]
fn a_write_to_a_minio_store_whose_input_is_missing_exits_6_naming_it_before_any_request() {
    let system = keyed(vault("# A\n"));
    let written = arky(
        &system,
        &["write", "b.md", "-s", UNREACHABLE_MINIO, "-i", "missing.md"],
        b"",
    );
    failed(&written, 6, "arky: missing.md: no file at this path\n");
    hides_the_keys(&written);
}

#[cfg(feature = "minio")]
#[test]
fn with_a_minio_store_the_path_is_checked_before_any_request() {
    let system = keyed(vault("# A\n"));
    for path in ["/notes/a.md", "./a.md", "notes/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", UNREACHABLE_MINIO], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path\n"),
        );
    }
    let fixed = arky(&system, &["fix", "./notes", "-s", UNREACHABLE_MINIO], b"");
    failed(&fixed, 2, "arky: ./notes: `./notes` is not a store path\n");
}

#[cfg(feature = "minio")]
#[test]
fn asset_finish_deletions_of_a_minio_store_exits_2_naming_it_before_any_request() {
    let system = keyed(vault("# A\n"));
    let ran = arky(
        &system,
        &["asset", "finish-deletions", "--store", UNREACHABLE_MINIO],
        b"",
    );
    failed(
        &ran,
        2,
        "arky: `minio+http://${KEY}:${SECRET}@127.0.0.1:1/notes` keeps no deletions: asset finish-deletions takes a MongoDB store\n",
    );
    hides_the_keys(&ran);
}

#[cfg(not(feature = "mongodb"))]
#[test]
fn a_mongodb_store_exits_3_in_a_build_without_it() {
    let system = vault("# A\n");
    let store = "mongodb://127.0.0.1:1/notes/files?files=x";
    let ran = arky(&system, &["get", "a.md", "-s", store], b"");
    failed(
        &ran,
        3,
        "arky: `mongodb://127.0.0.1:1/notes/files?files=x`: no backend for mongodb in this build\n",
    );
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_server_that_cannot_be_reached_exits_4_naming_the_store() {
    let system = keyed(vault("# A\n"));
    let line = format!("arky: `{UNREACHABLE_MONGODB}`: ");
    let got = arky(&system, &["get", "a.md", "-s", UNREACHABLE_MONGODB], b"");
    failed(&got, 4, &line);
    let written = arky(
        &system,
        &["write", "b.md", "-s", UNREACHABLE_MONGODB],
        b"# B\n",
    );
    failed(&written, 4, &line);
    let finish = ["asset", "finish-deletions", "-s", UNREACHABLE_MONGODB];
    failed(&arky(&system, &finish, b""), 4, &line);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "mongodb")]
#[test]
fn with_a_mongodb_store_the_path_is_checked_before_connecting() {
    let system = keyed(vault("# A\n"));
    for path in ["/notes/a.md", "./a.md", "notes/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", UNREACHABLE_MONGODB], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path\n"),
        );
    }
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_store_with_no_files_store_exits_2_naming_it() {
    let system = keyed(vault("# A\n"));
    let store = "mongodb://127.0.0.1:1/notes/files";
    let ran = arky(&system, &["get", "a.md", "-s", store], b"");
    failed(
        &ran,
        2,
        "arky: `mongodb://127.0.0.1:1/notes/files`: files is required: the MinIO store of the collection's files\n",
    );
}

#[cfg(not(feature = "s3"))]
#[test]
fn an_s3_store_exits_3_in_a_build_without_it() {
    let system = vault("# A\n");
    let store = "s3://notes/drafts?region=eu-west-1";
    let ran = arky(&system, &["write", "b.md", "-s", store], b"# B\n");
    failed(
        &ran,
        3,
        "arky: `s3://notes/drafts?region=eu-west-1`: no backend for s3 in this build\n",
    );
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "s3")]
#[test]
fn an_s3_store_with_no_region_exits_2_and_with_no_keys_exits_3_naming_it() {
    let system = vault("# A\n");
    let no_region = arky(&system, &["get", "a.md", "-s", "s3://notes"], b"");
    failed(
        &no_region,
        2,
        "arky: `s3://notes`: no region: s3 needs ?region=<region>\n",
    );
    let store = "s3://notes/vault?region=eu-west-1";
    let line = "arky: `s3://notes/vault?region=eu-west-1`: AWS_ACCESS_KEY_ID is not set: it holds the AWS access key\n";
    failed(&arky(&system, &["get", "a.md", "-s", store], b""), 3, line);
    failed(
        &arky(&system, &["write", "b.md", "-s", store], b"# B\n"),
        3,
        line,
    );
    failed(&arky(&system, &["fix", "-s", store], b""), 3, line);
    assert!(!system.exists(Path::new("/vault/b.md")).unwrap());
}

#[cfg(feature = "s3")]
#[test]
fn with_an_s3_store_the_path_is_checked_before_any_request() {
    let system = keyed(vault("# A\n"));
    let store = "s3://notes/vault?region=eu-west-1";
    for path in ["/notes/a.md", "./a.md", "notes/../a.md"] {
        let ran = arky(&system, &["get", path, "-s", store], b"");
        failed(
            &ran,
            2,
            &format!("arky: {path}: `{path}` is not a store path\n"),
        );
    }
    let finish = arky(&system, &["asset", "finish-deletions", "-s", store], b"");
    failed(
        &finish,
        2,
        "arky: `s3://notes/vault?region=eu-west-1` keeps no deletions: asset finish-deletions takes a MongoDB store\n",
    );
    hides_the_keys(&finish);
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
            "arky: /vault/arky.toml: {key} in [lock] is not a whole number of seconds above zero\n"
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
        "arky: /vault/arky.toml: [lock] has no key expire_seconds; it takes expires_seconds and wait_seconds\n",
    );
}

#[test]
fn a_lock_that_is_not_a_table_exits_3() {
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", b"lock = 5\n")
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 3, "arky: /vault/arky.toml: lock is not a table\n");
}

#[test]
fn a_configuration_file_the_reader_cannot_read_exits_3_with_the_readers_error() {
    let system = vault("# A\n")
        .with_file("/vault/arky.toml", b"[lock\n")
        .unwrap();
    let ran = arky(&system, &["get", "a.md"], b"");
    failed(&ran, 3, "arky: /vault/arky.toml: ");
    let bad = vault("# A\n")
        .with_file("/vault/arky.toml", &[0xff, 0xfe])
        .unwrap();
    let unread = arky(&bad, &["get", "a.md"], b"");
    failed(&unread, 3, "arky: /vault/arky.toml: ");
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
    failed(&ran, 3, "arky: /vault/arky.toml: ");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_with_a_store_removes_its_expired_locks_and_writes_their_store_paths() {
    let whole = locked(6, &[]);
    let all = arky(&whole, &["fix", "-s", "/vault"], b"");
    wrote_lines(&all, "notes/report.md.arky-lock\n");
    assert!(!whole.exists(Path::new(LOCK)).unwrap());
    let under = locked(6, &[]);
    let in_folder = arky(&under, &["fix", "notes", "-s", "/vault"], b"");
    wrote_lines(&in_folder, "notes/report.md.arky-lock\n");
    let saved = locked(
        6,
        &[("/arky.toml", "[stores.v]\nurl = \"file:///vault\"\n")],
    );
    let named = arky(&saved, &["fix", "-s", "arky://v/notes"], b"");
    wrote_lines(&named, "report.md.arky-lock\n");
    assert!(!saved.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_with_a_store_and_a_path_that_is_not_a_store_path_exits_2_and_removes_nothing() {
    let system = locked(6, &[]);
    let ran = arky(&system, &["fix", "./notes", "-s", "/vault"], b"");
    failed(&ran, 2, "arky: ./notes: `./notes` is not a store path\n");
    assert!(system.exists(Path::new(LOCK)).unwrap());
}

#[test]
fn fix_without_a_store_works_on_the_file_system_whatever_the_default_and_arky_store() {
    let config = "default = \"elsewhere\"\n[stores.elsewhere]\nurl = \"file:///elsewhere\"\n";
    let system = locked(6, &[("/arky.toml", config)])
        .with_env("ARKY_STORE", "arky://elsewhere")
        .unwrap();
    let ran = arky(&system, &["fix", "notes"], b"");
    wrote_lines(&ran, "notes/report.md.arky-lock\n");
}

#[test]
fn the_lock_comes_from_the_nearest_arky_toml_up_the_tree_that_sets_each_key() {
    let far = (
        "/arky.toml",
        "[lock]\nexpires_seconds = 60\nwait_seconds = 7\n",
    );
    let system = locked(0, &[far]);
    failed(&arky(&system, &SAVE, b""), 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&system), Duration::from_secs(7));
    let near = ("/vault/arky.toml", "[lock]\nwait_seconds = 3\n");
    let nearer = locked(0, &[far, near]);
    failed(&arky(&nearer, &SAVE, b""), 4, "arky: notes/report.md: ");
    assert_eq!(since_start(&nearer), Duration::from_secs(3));
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
