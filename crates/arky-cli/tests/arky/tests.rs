#[cfg(any(feature = "minio", feature = "mongodb"))]
use std::ffi::OsStr;
use std::fs::{self, File};
#[cfg(any(feature = "minio", feature = "mongodb"))]
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};

use arky::bundle::Missing;
use arky::forms::{Form, decode};
use arky::parser::ast::{FrontmatterFormat, RawDocument};
use arky::parser::{from_json, parse, to_json};
use arky::writer::{WriteOptions, write};
use assert_cmd::Command;
use assert_cmd::assert::{Assert, OutputAssertExt as _};
use assert_cmd::cargo::cargo_bin;
use predicates::ord::eq;
use predicates::str::{is_empty, starts_with};
use tempfile::TempDir;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../arky/src/parser/tests/fixtures"
);

const FORMS: &[&str] = &[
    "markdown",
    "json",
    #[cfg(feature = "binary-format")]
    "binary",
    "zip",
];

/// The four variables a `MinIO` store reads: a server no process listens on,
/// whose connection is refused at once, and keys no output may show.
#[cfg(any(feature = "minio", feature = "mongodb"))]
const MINIO_VARIABLES: [(&str, &str); 4] = [
    ("ARKY_MINIO_ENDPOINT", "http://127.0.0.1:1"),
    ("ARKY_MINIO_REGION", "us-east-1"),
    ("AWS_ACCESS_KEY_ID", "arky-binary-access"),
    ("AWS_SECRET_ACCESS_KEY", "arky-binary-secret"),
];

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n not a real picture";

const REPORT: &str = "# Report\n\n![Logo](media/logo.png)\n";

/// A server no process listens on, which the driver tries once and gives up
/// on at once: no server selection to wait for, no retry.
#[cfg(feature = "mongodb")]
const UNREACHABLE: &str = "mongodb://127.0.0.1:1/?loadBalanced=true&retryReads=false";

/// Runs the binary in `folder` with `arguments`, `stdin` on standard input.
fn arky(folder: &Path, arguments: &[&str], stdin: &[u8]) -> Assert {
    Command::cargo_bin("arky")
        .unwrap()
        .current_dir(folder)
        .args(arguments)
        .write_stdin(stdin)
        .assert()
}

/// Runs the binary in `folder` with `arguments`, `ARKY_MONGODB_URI` set to
/// `uri` or removed when it is `None`, a bucket, and `MINIO_VARIABLES`.
#[cfg(feature = "mongodb")]
fn arky_with_uri(folder: &Path, arguments: &[&str], uri: Option<&OsStr>) -> Assert {
    let mut command = Command::cargo_bin("arky").unwrap();
    command.current_dir(folder).args(arguments);
    command.env("ARKY_MONGODB_BUCKET", "arky-files");
    command.envs(MINIO_VARIABLES);
    if let Some(value) = uri {
        command.env("ARKY_MONGODB_URI", value);
    } else {
        command.env_remove("ARKY_MONGODB_URI");
    }
    command.write_stdin(b"# B\n".as_slice()).assert()
}

/// Runs the binary in `folder` with `arguments`, `# B` on standard input,
/// and of the variables of `MINIO_VARIABLES` only those in `variables`.
#[cfg(feature = "minio")]
fn arky_with_minio(folder: &Path, arguments: &[&str], variables: &[(&str, &OsStr)]) -> Assert {
    let mut command = Command::cargo_bin("arky").unwrap();
    command.current_dir(folder).args(arguments);
    for (name, _) in MINIO_VARIABLES {
        command.env_remove(name);
    }
    command.envs(variables.iter().copied());
    command.write_stdin(b"# B\n".as_slice()).assert()
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

/// Asserts the run failed with `code` and one line on standard error that
/// opens with `line`, and wrote nothing to standard output.
fn failed(ran: Assert, code: i32, line: &str) {
    let stderr = String::from_utf8(ran.get_output().stderr.clone()).unwrap();
    ran.code(code).stdout(is_empty()).stderr(starts_with(line));
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.ends_with('\n'), "{stderr}");
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

/// Asserts as `failed` does, and that standard error shows neither key of
/// `MINIO_VARIABLES`.
#[cfg(feature = "minio")]
fn failed_hiding_the_keys(ran: Assert, code: i32, line: &str) {
    let stderr = String::from_utf8(ran.get_output().stderr.clone()).unwrap();
    for (_, key) in &MINIO_VARIABLES[2..] {
        assert!(!stderr.contains(key), "{stderr}");
    }
    failed(ran, code, line);
}

/// A new folder under the target folder.
fn folder() -> TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// The names in `folder`, sorted.
fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort_unstable();
    names
}

/// Takes the write permission off `folder`.
fn read_only(folder: &Path) {
    let mut permissions = fs::metadata(folder).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(folder, permissions).unwrap();
}

/// Writes `REPORT` at `path` under `root`, and `PNG` beside it at
/// `media/logo.png`.
fn report(root: &Path, path: &str) {
    let folder = root.join(path).parent().unwrap().to_path_buf();
    fs::create_dir_all(folder.join("media")).unwrap();
    fs::write(root.join(path), REPORT).unwrap();
    fs::write(folder.join("media/logo.png"), PNG).unwrap();
}

/// Asserts `root` holds `REPORT` at `path` and `PNG` beside it at
/// `media/logo.png`.
fn reported(root: &Path, path: &str) {
    assert_eq!(
        fs::read_to_string(root.join(path)).unwrap(),
        REPORT,
        "{path}"
    );
    let logo = root.join(path).parent().unwrap().join("media/logo.png");
    assert_eq!(fs::read(logo).unwrap(), PNG, "{path}");
}

fn stdout(ran: &Assert) -> Vec<u8> {
    ran.get_output().stdout.clone()
}

#[test]
fn the_copy_of_each_fixture_in_each_form_is_arkys_form_and_a_second_copy_changes_nothing() {
    let folder = folder();
    let root = folder.path();
    // The picture `obsidian_note.md` embeds.
    fs::write(root.join("tree-sketch.png"), PNG).unwrap();
    for (name, source) in fixtures() {
        fs::write(root.join(&name), &source).unwrap();
        let arky_form = arkys_form(&source, FrontmatterFormat::Yaml);
        let json_form = arkys_form_through_json(&source);
        for form in FORMS.iter().copied() {
            let expected = if form == "json" {
                &json_form
            } else {
                &arky_form
            };
            let first = format!("{form}/{name}");
            let second = format!("{form}/again/{name}");
            let got = arky(root, &["get", &name, "-f", form], b"").success();
            arky(root, &["write", &first, "-f", form], &stdout(&got))
                .success()
                .stdout(is_empty())
                .stderr(is_empty());
            let copied = fs::read_to_string(root.join(&first)).unwrap();
            assert_eq!(&copied, expected, "{form}: {name}");
            let again = arky(root, &["get", &first, "-f", form], b"").success();
            arky(root, &["write", &second, "-f", form], &stdout(&again)).success();
            assert_eq!(fs::read_to_string(root.join(&second)).unwrap(), copied);
        }
    }
}

#[test]
fn a_pipe_between_two_runs_copies_the_file_in_each_form() {
    let folder = folder();
    let root = folder.path();
    let source = fs::read_to_string(Path::new(FIXTURES).join("hugo_post.md")).unwrap();
    fs::write(root.join("a.md"), &source).unwrap();
    for form in FORMS.iter().copied() {
        let target = format!("{form}.md");
        let mut get = Process::new(cargo_bin("arky"))
            .current_dir(root)
            .args(["get", "a.md", "-f", form])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pipe = get.stdout.take().unwrap();
        let written = Process::new(cargo_bin("arky"))
            .current_dir(root)
            .args(["write", &target, "-f", form])
            .stdin(pipe)
            .status()
            .unwrap();
        assert!(get.wait().unwrap().success());
        assert!(written.success());
        let copied = fs::read_to_string(root.join(&target)).unwrap();
        let expected = if form == "json" {
            arkys_form_through_json(&source)
        } else {
            arkys_form(&source, FrontmatterFormat::Yaml)
        };
        assert_eq!(copied, expected, "{form}");
    }
}

#[test]
fn toml_frontmatter_on_get_and_on_write_and_yaml_when_left_out() {
    let folder = folder();
    let root = folder.path();
    let jekyll = fs::read_to_string(Path::new(FIXTURES).join("jekyll_post.md")).unwrap();
    let zola = fs::read_to_string(Path::new(FIXTURES).join("zola_page.md")).unwrap();
    assert!(jekyll.starts_with("---\n") && zola.starts_with("+++\n"));
    fs::write(root.join("jekyll.md"), &jekyll).unwrap();
    fs::write(root.join("zola.md"), &zola).unwrap();
    let toml_form = arkys_form(&jekyll, FrontmatterFormat::Toml);
    arky(root, &["get", "jekyll.md", "--frontmatter", "toml"], b"")
        .success()
        .stdout(eq(toml_form.as_bytes()));
    arky(root, &["get", "zola.md"], b"")
        .success()
        .stdout(starts_with("---\n"));
    arky(
        root,
        &["write", "toml.md", "--frontmatter", "toml"],
        jekyll.as_bytes(),
    )
    .success();
    assert_eq!(fs::read_to_string(root.join("toml.md")).unwrap(), toml_form);
    arky(root, &["write", "yaml.md"], zola.as_bytes()).success();
    let yaml_file = fs::read_to_string(root.join("yaml.md")).unwrap();
    assert!(yaml_file.starts_with("---\n"), "{yaml_file}");
}

#[test]
fn output_and_input_name_files_of_the_machine() {
    let folder = folder();
    let root = folder.path();
    let source = fs::read_to_string(Path::new(FIXTURES).join("recipe.md")).unwrap();
    fs::write(root.join("a.md"), &source).unwrap();
    let streamed = arky(root, &["get", "a.md", "-f", "json"], b"").success();
    arky(
        root,
        &["get", "a.md", "--format", "json", "--output", "out.json"],
        b"",
    )
    .success()
    .stdout(is_empty())
    .stderr(is_empty());
    assert_eq!(fs::read(root.join("out.json")).unwrap(), stdout(&streamed));
    arky(
        root,
        &["write", "b.md", "-f", "json", "-i", "out.json"],
        b"",
    )
    .success();
    let copied = fs::read_to_string(root.join("b.md")).unwrap();
    assert_eq!(copied, arkys_form(&source, FrontmatterFormat::Yaml));
}

#[test]
fn a_failing_get_with_output_leaves_no_file_and_no_partial_file() {
    let folder = folder();
    let root = folder.path();
    fs::write(root.join("a.md"), "# A\n").unwrap();
    let missing = arky(root, &["get", "missing.md", "-o", "out.md"], b"");
    failed(missing, 6, "arky: missing.md: no file at this path");
    assert_eq!(names(root), ["a.md"]);
    let locked = root.join("locked");
    fs::create_dir_all(&locked).unwrap();
    read_only(&locked);
    let refused = arky(root, &["get", "a.md", "-o", "locked/out.md"], b"");
    failed(refused, 4, "arky: locked/out.md: ");
    assert_eq!(names(&locked), Vec::<String>::new());
}

#[test]
fn a_missing_path_exits_6_with_one_line_naming_the_path() {
    let folder = folder();
    let ran = arky(folder.path(), &["get", "missing.md"], b"");
    ran.code(6_i32)
        .stdout(is_empty())
        .stderr(eq("arky: missing.md: no file at this path\n"));
}

#[test]
fn text_that_is_not_json_exits_1_and_stores_nothing() {
    let folder = folder();
    let root = folder.path();
    let ran = arky(root, &["write", "b.md", "-f", "json"], b"nope\n");
    failed(ran, 1, "arky: b.md: ");
    assert_eq!(names(root), Vec::<String>::new());
}

#[test]
fn an_unknown_option_and_a_missing_path_exit_2() {
    let folder = folder();
    let root = folder.path();
    arky(root, &["get", "a.md", "--nope"], b"")
        .code(2_i32)
        .stdout(is_empty());
    arky(root, &["get"], b"").code(2_i32).stdout(is_empty());
}

#[cfg(not(feature = "mongodb"))]
#[test]
fn a_mongodb_store_exits_3_in_a_build_without_it() {
    let folder = folder();
    let root = folder.path();
    fs::write(root.join("a.md"), "# A\n").unwrap();
    arky(
        root,
        &["get", "a.md", "--store", "mongodb:notes/files"],
        b"",
    )
    .code(3_i32)
    .stdout(is_empty())
    .stderr(eq(
        "arky: mongodb:notes/files: this arky is built without the MongoDB store\n",
    ));
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_store_with_no_connection_string_exits_3_with_one_line_naming_the_variable() {
    let folder = folder();
    let root = folder.path();
    fs::write(root.join("a.md"), "# A\n").unwrap();
    let get = ["get", "a.md", "--store", "mongodb:notes/files"];
    arky_with_uri(root, &get, None)
        .code(3_i32)
        .stdout(is_empty())
        .stderr(eq(
            "arky: ARKY_MONGODB_URI is not set: it holds the MongoDB connection string\n",
        ));
    let not_utf8 = OsStr::from_bytes(b"mongodb://\xff");
    arky_with_uri(
        root,
        &["write", "b.md", "-s", "mongodb:notes/files"],
        Some(not_utf8),
    )
    .code(3_i32)
    .stdout(is_empty())
    .stderr(eq(
        "arky: ARKY_MONGODB_URI is not UTF-8: it holds the MongoDB connection string\n",
    ));
    assert_eq!(names(root), ["a.md"]);
}

#[cfg(feature = "mongodb")]
#[test]
fn a_mongodb_server_that_cannot_be_reached_exits_4_naming_the_store() {
    let folder = folder();
    let root = folder.path();
    let uri = Some(OsStr::new(UNREACHABLE));
    let get = arky_with_uri(root, &["get", "a.md", "-s", "mongodb:notes/files"], uri);
    failed(get, 4, "arky: mongodb:notes/files: ");
    let write = arky_with_uri(root, &["write", "b.md", "-s", "mongodb:notes/files"], uri);
    failed(write, 4, "arky: mongodb:notes/files: ");
    assert_eq!(names(root), Vec::<String>::new());
}

#[cfg(not(feature = "minio"))]
#[test]
fn a_minio_store_exits_3_in_a_build_without_it() {
    let folder = folder();
    let root = folder.path();
    arky(
        root,
        &["write", "b.md", "-s", "minio:notes/drafts"],
        b"# B\n",
    )
    .code(3_i32)
    .stdout(is_empty())
    .stderr(eq(
        "arky: minio:notes/drafts: this arky is built without the MinIO store\n",
    ));
    assert_eq!(names(root), Vec::<String>::new());
}

#[cfg(feature = "minio")]
#[test]
fn each_minio_variable_left_out_or_not_utf8_exits_3_with_one_line_naming_it() {
    let folder = folder();
    let root = folder.path();
    fs::write(root.join("a.md"), "# A\n").unwrap();
    let all: Vec<(&str, &OsStr)> = MINIO_VARIABLES
        .iter()
        .map(|(name, value)| (*name, OsStr::new(value)))
        .collect();
    let lines = [
        "arky: ARKY_MINIO_ENDPOINT is not set: it holds the MinIO server's address\n",
        "arky: ARKY_MINIO_REGION is not set: it holds the region the MinIO server expects\n",
        "arky: AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key\n",
        "arky: AWS_SECRET_ACCESS_KEY is not set: it holds the MinIO secret key\n",
    ];
    for (index, line) in lines.iter().enumerate() {
        let mut variables = all.clone();
        variables.remove(index);
        let get = ["get", "a.md", "--store", "minio:notes/vault"];
        failed_hiding_the_keys(arky_with_minio(root, &get, &variables), 3, line);
        let write = ["write", "b.md", "-s", "minio:notes/vault"];
        failed_hiding_the_keys(arky_with_minio(root, &write, &variables), 3, line);
    }
    let not_utf8 = OsStr::from_bytes(b"http://\xff");
    let mut endpoint = all.clone();
    endpoint[0] = ("ARKY_MINIO_ENDPOINT", not_utf8);
    failed_hiding_the_keys(
        arky_with_minio(root, &["get", "a.md", "-s", "minio:notes/vault"], &endpoint),
        3,
        "arky: ARKY_MINIO_ENDPOINT is not UTF-8: it holds the MinIO server's address\n",
    );
    let mut secret = all;
    secret[3] = ("AWS_SECRET_ACCESS_KEY", OsStr::from_bytes(b"\xff"));
    failed_hiding_the_keys(
        arky_with_minio(root, &["write", "b.md", "-s", "minio:notes/vault"], &secret),
        3,
        "arky: AWS_SECRET_ACCESS_KEY is not UTF-8: it holds the MinIO secret key\n",
    );
    assert_eq!(names(root), ["a.md"]);
}

#[cfg(feature = "minio")]
#[test]
fn a_minio_server_that_cannot_be_reached_exits_4_naming_the_path_on_a_get_and_the_store_on_a_write()
{
    let folder = folder();
    let root = folder.path();
    let variables: Vec<(&str, &OsStr)> = MINIO_VARIABLES
        .iter()
        .map(|(name, value)| (*name, OsStr::new(value)))
        .collect();
    let get = arky_with_minio(root, &["get", "a.md", "-s", "minio:notes"], &variables);
    failed_hiding_the_keys(get, 4, "arky: a.md: ");
    let write = ["write", "b.md", "-s", "minio:notes/vault"];
    let written = arky_with_minio(root, &write, &variables);
    failed_hiding_the_keys(written, 4, "arky: minio:notes/vault: ");
    assert_eq!(names(root), Vec::<String>::new());
}

#[test]
fn a_write_into_a_folder_that_is_not_writable_exits_4() {
    let folder = folder();
    let root = folder.path();
    let locked = root.join("locked");
    fs::create_dir_all(&locked).unwrap();
    read_only(&locked);
    let ran = arky(root, &["write", "locked/b.md"], b"# B\n");
    failed(ran, 4, "arky: locked/b.md: ");
    assert_eq!(names(&locked), Vec::<String>::new());
}

#[test]
fn a_standard_input_that_cannot_be_read_exits_4() {
    let folder = folder();
    let root = folder.path();
    let ran = Process::new(cargo_bin("arky"))
        .current_dir(root)
        .args(["write", "b.md"])
        .stdin(File::open(root).unwrap())
        .output()
        .unwrap()
        .assert();
    failed(ran, 4, "arky: standard input: ");
    assert_eq!(names(root), Vec::<String>::new());
}

#[test]
fn a_relative_path_from_a_subfolder_names_the_file_the_shell_names() {
    let folder = folder();
    let root = folder.path();
    let source = fs::read_to_string(Path::new(FIXTURES).join("task.md")).unwrap();
    fs::create_dir_all(root.join("x")).unwrap();
    fs::create_dir_all(root.join("y")).unwrap();
    fs::write(root.join("x/a.md"), &source).unwrap();
    let expected = arkys_form(&source, FrontmatterFormat::Yaml);
    let subfolder: PathBuf = root.join("y");
    arky(&subfolder, &["get", "../x/a.md"], b"")
        .success()
        .stdout(eq(expected.as_bytes()));
    arky(&subfolder, &["write", "../x/b.md"], source.as_bytes()).success();
    assert_eq!(fs::read_to_string(root.join("x/b.md")).unwrap(), expected);
}

#[test]
fn help_and_version_exit_0() {
    let folder = folder();
    let root = folder.path();
    arky(root, &["--version"], b"").success().stdout(eq(concat!(
        "arky ",
        env!("CARGO_PKG_VERSION"),
        "\n"
    )));
    arky(root, &["--help"], b"")
        .success()
        .stdout(starts_with("Markdown documents as a typed tree\n"));
}

#[test]
fn a_markdown_input_brings_each_file_it_embeds_and_a_markdown_output_writes_each_beside_it() {
    let folder = folder();
    let root = folder.path();
    report(root, "draft/report.md");
    arky(
        root,
        &["write", "notes/report.md", "-i", "draft/report.md"],
        b"",
    )
    .success()
    .stdout(is_empty())
    .stderr(is_empty());
    reported(root, "notes/report.md");
    arky(
        root,
        &["get", "notes/report.md", "-o", "out/report.md"],
        b"",
    )
    .success()
    .stdout(is_empty())
    .stderr(is_empty());
    reported(root, "out/report.md");
    assert_eq!(names(&root.join("out")), ["media", "report.md"]);
}

#[test]
fn an_embedded_output_is_one_file_holding_each_embed_as_a_named_data_uri() {
    let folder = folder();
    let root = folder.path();
    report(root, "notes/report.md");
    arky(
        root,
        &["get", "notes/report.md", "-o", "out/one.md", "--embed"],
        b"",
    )
    .success()
    .stdout(is_empty());
    assert_eq!(names(&root.join("out")), ["one.md"]);
    let one = fs::read_to_string(root.join("out/one.md")).unwrap();
    assert!(
        one.contains("![Logo](data:image/png;name=media/logo.png;base64,"),
        "{one}"
    );
    arky(root, &["get", "notes/report.md"], b"")
        .success()
        .stdout(eq(one.as_bytes()));
}

#[test]
fn a_pipe_between_two_runs_carries_each_file_to_the_copy_in_each_form() {
    let folder = folder();
    let root = folder.path();
    report(root, "notes/report.md");
    for form in FORMS.iter().copied() {
        let copy = format!("{form}/report.md");
        let mut get = Process::new(cargo_bin("arky"))
            .current_dir(root)
            .args(["get", "notes/report.md", "-f", form])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pipe = get.stdout.take().unwrap();
        let written = Process::new(cargo_bin("arky"))
            .current_dir(root)
            .args(["write", &copy, "-f", form])
            .stdin(pipe)
            .status()
            .unwrap();
        assert!(get.wait().unwrap().success(), "{form}");
        assert!(written.success(), "{form}");
        reported(root, &copy);
    }
}

#[test]
fn a_zip_output_holds_index_md_and_each_file_and_reads_back_as_input() {
    let folder = folder();
    let root = folder.path();
    report(root, "notes/report.md");
    arky(
        root,
        &["get", "notes/report.md", "-f", "zip", "-o", "r.zip"],
        b"",
    )
    .success()
    .stdout(is_empty());
    let bundle = decode(
        &fs::read(root.join("r.zip")).unwrap(),
        Form::Zip,
        Missing::Refuse,
    )
    .unwrap();
    assert_eq!(bundle.files(), ["media/logo.png"]);
    arky(
        root,
        &["write", "z/report.md", "-f", "zip", "-i", "r.zip"],
        b"",
    )
    .success();
    reported(root, "z/report.md");
}

#[test]
fn a_missing_embedded_file_fails_with_code_1_and_allow_missing_stores_and_gives_it() {
    let folder = folder();
    let root = folder.path();
    let line = "arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it\n";
    let text = b"![Logo](media/none.png)\n";
    arky(root, &["write", "notes/a.md"], text)
        .code(1_i32)
        .stdout(is_empty())
        .stderr(eq(line));
    assert_eq!(names(root), Vec::<String>::new());
    arky(root, &["write", "notes/a.md", "--allow-missing"], text).success();
    assert_eq!(fs::read(root.join("notes/a.md")).unwrap(), text);
    arky(root, &["get", "notes/a.md"], b"")
        .code(1_i32)
        .stdout(is_empty())
        .stderr(eq(line));
    arky(root, &["get", "notes/a.md", "--allow-missing"], b"")
        .success()
        .stdout(eq(text.as_slice()));
}

#[test]
fn embed_without_an_output_and_embed_with_another_form_exit_2_and_write_nothing() {
    let folder = folder();
    let root = folder.path();
    report(root, "notes/report.md");
    let parsed = arky(root, &["get", "notes/report.md", "--embed"], b"");
    let usage = String::from_utf8(parsed.get_output().stderr.clone()).unwrap();
    parsed.code(2_i32).stdout(is_empty());
    assert!(usage.contains("--output <FILE>"), "{usage}");
    arky(
        root,
        &[
            "get",
            "notes/report.md",
            "-f",
            "zip",
            "-o",
            "r.zip",
            "--embed",
        ],
        b"",
    )
    .code(2_i32)
    .stdout(is_empty())
    .stderr(eq("arky: --embed writes markdown; it needs -f markdown\n"));
    assert_eq!(names(root), ["notes"]);
}
