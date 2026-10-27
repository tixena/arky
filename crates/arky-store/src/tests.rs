//! Store paths, the path of a name beside one, and the messages of store
//! errors.

use crate::{StoreError, StorePath};

#[test]
fn a_path_of_parts_is_a_store_path() {
    for text in [
        "a.md",
        "notes/task.md",
        ".obsidian/x.md",
        "..a/b.",
        "a b/c d.md",
    ] {
        assert_eq!(StorePath::new(text).unwrap().as_str(), text);
    }
}

#[test]
fn a_path_that_breaks_a_rule_is_invalid() {
    let broken = [
        "",
        "a//b",
        "/a",
        "a/",
        "/",
        "a\\b",
        "./a",
        ".",
        "..",
        "a/./b",
        "notes/../secret.md",
    ];
    for text in broken {
        let refused = Err(StoreError::InvalidPath(text.to_owned()));
        assert_eq!(StorePath::new(text), refused, "{text:?}");
    }
}

#[test]
fn a_name_beside_a_path_is_its_path_from_the_folder_of_the_path() {
    let cases = [
        ("notes/report.md", "media/logo.png", "notes/media/logo.png"),
        ("report.md", "logo.png", "logo.png"),
        ("a/b/c.md", "parts/d.md", "a/b/parts/d.md"),
    ];
    for (path, name, beside) in cases {
        let found = StorePath::new(path).unwrap().beside(name).unwrap();
        assert_eq!(found.as_str(), beside, "{path} {name}");
    }
}

#[test]
fn a_name_beside_a_path_that_breaks_a_rule_is_invalid() {
    let path = StorePath::new("notes/report.md").unwrap();
    for (name, beside) in [
        ("../x.png", "notes/../x.png"),
        ("a\\b.png", "notes/a\\b.png"),
    ] {
        let refused = Err(StoreError::InvalidPath(beside.to_owned()));
        assert_eq!(path.beside(name), refused, "{name:?}");
    }
}

#[test]
fn an_error_names_its_path() {
    let path = StorePath::new("notes/none.md").unwrap();
    let missing = StoreError::NotFound(path.clone()).to_string();
    assert_eq!(missing, "notes/none.md: no file at this path");
    let conflict = StoreError::Conflict(path).to_string();
    assert_eq!(
        conflict,
        "notes/none.md: the file changed, or a file is already there"
    );
    let invalid = StoreError::InvalidPath("a//b".to_owned()).to_string();
    assert_eq!(invalid, "`a//b` is not a store path");
}
