use arky::forms::Form;
use clap::error::ErrorKind;
use clap::{CommandFactory as _, Parser as _};

use super::{AssetCommand, Cli, Command, FormatArg, StoreArg, StoreCommand, SyntaxArg};

#[test]
fn the_command_line_is_well_formed() {
    Cli::command().debug_assert();
}

#[test]
fn markdown_and_yaml_are_the_defaults_no_store_is_the_file_system_and_no_flag_is_set() {
    let get = Cli::try_parse_from(["arky", "get", "a.md"]).unwrap();
    assert!(
        matches!(
            &get.command,
            Command::Get {
                allow_missing: false,
                embed: false,
                format: FormatArg::Markdown,
                frontmatter: SyntaxArg::Yaml,
                output: None,
                path,
                store: None,
            } if path.as_os_str() == "a.md"
        ),
        "{get:?}"
    );
    let write = Cli::try_parse_from(["arky", "write", "b.md"]).unwrap();
    assert!(
        matches!(
            &write.command,
            Command::Write {
                allow_missing: false,
                format: FormatArg::Markdown,
                frontmatter: SyntaxArg::Yaml,
                input: None,
                path,
                store: None,
            } if path.as_os_str() == "b.md"
        ),
        "{write:?}"
    );
}

#[test]
fn the_short_and_long_options_parse() {
    let get = Cli::try_parse_from([
        "arky",
        "get",
        "a.md",
        "-f",
        "zip",
        "--frontmatter",
        "toml",
        "-o",
        "a.bin",
        "-s",
        "arky://notes",
        "--allow-missing",
    ])
    .unwrap();
    assert!(
        matches!(
            &get.command,
            Command::Get {
                allow_missing: true,
                embed: false,
                format: FormatArg::Zip,
                frontmatter: SyntaxArg::Toml,
                output: Some(output),
                path: _,
                store: Some(StoreArg(store)),
            } if output.as_os_str() == "a.bin" && store == "arky://notes"
        ),
        "{get:?}"
    );
    let write = Cli::try_parse_from([
        "arky",
        "write",
        "b.md",
        "--format",
        "json",
        "--input",
        "a.json",
        "--store",
        "s3://notes/vault?region=eu-west-1",
        "--allow-missing",
    ])
    .unwrap();
    assert!(
        matches!(
            &write.command,
            Command::Write {
                allow_missing: true,
                format: FormatArg::Json,
                frontmatter: SyntaxArg::Yaml,
                input: Some(input),
                path: _,
                store: Some(StoreArg(store)),
            } if input.as_os_str() == "a.json" && store == "s3://notes/vault?region=eu-west-1"
        ),
        "{write:?}"
    );
}

#[test]
fn each_of_the_four_forms_parses_on_get_and_write_and_names_its_form() {
    let forms = [
        #[cfg(feature = "binary-format")]
        ("binary", Form::Binary),
        ("json", Form::Json),
        ("markdown", Form::Markdown),
        ("zip", Form::Zip),
    ];
    for (text, form) in forms {
        for command in ["get", "write"] {
            let parsed = Cli::try_parse_from(["arky", command, "a.md", "-f", text]).unwrap();
            let format = match parsed.command {
                Command::Get {
                    allow_missing: _,
                    embed: _,
                    format,
                    frontmatter: _,
                    output: _,
                    path: _,
                    store: _,
                }
                | Command::Write {
                    allow_missing: _,
                    format,
                    frontmatter: _,
                    input: _,
                    path: _,
                    store: _,
                } => Some(format),
                Command::Asset { command: _ }
                | Command::Fix { path: _, store: _ }
                | Command::Store { command: _ } => None,
            };
            assert_eq!(format.map(Form::from), Some(form), "{command} {text}");
        }
    }
}

#[test]
fn fix_takes_an_optional_path_and_an_optional_store() {
    let bare = Cli::try_parse_from(["arky", "fix"]).unwrap();
    assert!(
        matches!(
            &bare.command,
            Command::Fix {
                path: None,
                store: None
            }
        ),
        "{bare:?}"
    );
    let named = Cli::try_parse_from(["arky", "fix", "notes", "-s", "arky://work/vault"]).unwrap();
    assert!(
        matches!(
            &named.command,
            Command::Fix {
                path: Some(path),
                store: Some(StoreArg(store)),
            } if path.as_os_str() == "notes" && store == "arky://work/vault"
        ),
        "{named:?}"
    );
}

#[test]
fn asset_finish_deletions_parses_its_store() {
    let finish = Cli::try_parse_from([
        "arky",
        "asset",
        "finish-deletions",
        "--store",
        "arky://docs",
    ])
    .unwrap();
    assert!(
        matches!(
            &finish.command,
            Command::Asset {
                command: AssetCommand::FinishDeletions {
                    store: StoreArg(store),
                },
            } if store == "arky://docs"
        ),
        "{finish:?}"
    );
}

#[test]
fn asset_finish_deletions_without_a_store_is_a_usage_error_with_code_2() {
    let error = Cli::try_parse_from(["arky", "asset", "finish-deletions"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    assert_eq!(error.exit_code(), 2_i32);
}

#[test]
fn store_list_parses_and_takes_no_argument_but_the_global_config() {
    let list = Cli::try_parse_from(["arky", "store", "list"]).unwrap();
    assert!(
        matches!(
            &list.command,
            Command::Store {
                command: StoreCommand::List
            }
        ),
        "{list:?}"
    );
    let mut command = Cli::command();
    command.build();
    let listed = command
        .find_subcommand("store")
        .and_then(|store| store.find_subcommand("list"))
        .unwrap();
    let mut taken: Vec<&str> = listed
        .get_arguments()
        .map(|argument| argument.get_id().as_str())
        .collect();
    taken.sort_unstable();
    assert_eq!(taken, ["config", "help"]);
    let error = Cli::try_parse_from(["arky", "store"]).unwrap_err();
    assert_eq!(error.exit_code(), 2_i32);
}

#[test]
fn embed_with_an_output_parses() {
    let get = Cli::try_parse_from(["arky", "get", "a.md", "-o", "out/a.md", "--embed"]).unwrap();
    assert!(
        matches!(
            &get.command,
            Command::Get {
                allow_missing: false,
                embed: true,
                format: FormatArg::Markdown,
                frontmatter: SyntaxArg::Yaml,
                output: Some(output),
                path: _,
                store: None,
            } if output.as_os_str() == "out/a.md"
        ),
        "{get:?}"
    );
}

#[test]
fn embed_without_an_output_is_a_usage_error_with_code_2() {
    for arguments in [
        ["arky", "get", "a.md", "--embed"].as_slice(),
        ["arky", "get", "a.md", "-f", "zip", "--embed"].as_slice(),
    ] {
        let error = Cli::try_parse_from(arguments).unwrap_err();
        assert_eq!(
            error.kind(),
            ErrorKind::MissingRequiredArgument,
            "{arguments:?}"
        );
        assert_eq!(error.exit_code(), 2_i32, "{arguments:?}");
    }
}

#[test]
fn any_store_text_parses_as_it_was_written_for_the_backends_to_read() {
    for text in [
        "",
        "notes",
        "./notes:drafts",
        "file:notes",
        "arky://notes/drafts",
        "minio+http://127.0.0.1:1/notes/drafts?region=us-east-1",
        "mongodb://h/notes/docs?files=arky%3A%2F%2Fmedia",
        "s3://notes?region=eu-west-1",
        "minio:notes",
        "${VAULT}",
    ] {
        let parsed = Cli::try_parse_from(["arky", "get", "a.md", "-s", text]).unwrap();
        assert!(
            matches!(
                &parsed.command,
                Command::Get {
                    allow_missing: _,
                    embed: _,
                    format: _,
                    frontmatter: _,
                    output: _,
                    path: _,
                    store: Some(StoreArg(store)),
                } if store == text
            ),
            "{parsed:?}"
        );
    }
}

#[test]
fn an_unknown_option_a_missing_path_and_an_unknown_value_are_usage_errors_with_code_2() {
    for arguments in [
        ["arky", "get", "a.md", "--nope"].as_slice(),
        ["arky", "get"].as_slice(),
        ["arky", "write"].as_slice(),
        ["arky", "get", "a.md", "-f", "bincode"].as_slice(),
        ["arky", "write", "b.md", "--frontmatter", "json"].as_slice(),
        ["arky", "write", "b.md", "-o", "b.json"].as_slice(),
        ["arky", "get", "a.md", "-i", "a.json"].as_slice(),
        ["arky", "write", "b.md", "-i", "b.md", "--embed"].as_slice(),
    ] {
        let error = Cli::try_parse_from(arguments).unwrap_err();
        assert_eq!(error.exit_code(), 2_i32, "{arguments:?}");
    }
}
