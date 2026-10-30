//! `arky.toml` files over `MemorySystem`: the walk up the tree and its merge,
//! one named file, and the error of each rule a file can break.

use core::time::Duration;
use std::path::{Path, PathBuf};

use os_shim::mock::MemorySystem;

use super::{Configuration, SavedStore, SavedStores};
use crate::StoreError;
use crate::lock::LockSettings;

const HOME: &str = r#"default = "vault_a"

[lock]
expires_seconds = 60
wait_seconds = 2

[stores.vault_a]
url = "file:a"

[stores.vault_b]
url = "file:b"
"#;

const VAULT: &str = r#"[lock]
wait_seconds = 10

[stores.vault_a]
url = "minio+https://minio.example.com/vault-a"
"#;

const WORK: &str = r#"[stores.vault_c]
url = "s3://acme-c?region=eu-west-1"
"#;

/// A `MemorySystem` holding each of `files`, a path and its text.
fn system(files: &[(&str, &str)]) -> MemorySystem {
    files
        .iter()
        .fold(MemorySystem::new(), |memory, (path, text)| {
            memory.with_file(path, text.as_bytes()).unwrap()
        })
}

/// The three files of the sample: in the home folder, in `work` and in
/// `work/vault`.
fn sample() -> MemorySystem {
    system(&[
        ("/home/me/arky.toml", HOME),
        ("/home/me/work/arky.toml", WORK),
        ("/home/me/work/vault/arky.toml", VAULT),
    ])
}

/// What `Configuration::discover` gives from `/home/me/work` with `text` as
/// `/home/me/arky.toml`.
fn discovered(text: &str) -> Result<Configuration, StoreError> {
    let files = system(&[("/home/me/arky.toml", text)]);
    Configuration::discover(&files, Path::new("/home/me/work"))
}

/// The error `NotConfigured` of `/home/me/arky.toml` with `what`.
fn home_refused(what: &str) -> Result<Configuration, StoreError> {
    Err(StoreError::NotConfigured(format!(
        "/home/me/arky.toml: {what}"
    )))
}

fn store(name: &str, url: &str, defined_in: &str) -> SavedStore {
    SavedStore {
        defined_in: PathBuf::from(defined_in),
        name: name.to_owned(),
        url: url.to_owned(),
    }
}

#[test]
fn the_files_up_the_tree_accumulate() {
    let configuration =
        Configuration::discover(&sample(), Path::new("/home/me/work/vault")).unwrap();
    let names: Vec<_> = configuration
        .stores
        .list()
        .map(|saved| (saved.name.as_str(), saved.defined_in.to_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        [
            ("vault_a", "/home/me/work/vault/arky.toml"),
            ("vault_b", "/home/me/arky.toml"),
            ("vault_c", "/home/me/work/arky.toml"),
        ]
    );
    assert_eq!(configuration.default.as_deref(), Some("vault_a"));
    assert_eq!(configuration.lock.expires, Duration::from_secs(60));
    assert_eq!(configuration.lock.wait, Duration::from_secs(10));
}

#[test]
fn a_name_defined_twice_is_the_nearer_store_whole() {
    let stores = SavedStores::discover(&sample(), Path::new("/home/me/work/vault")).unwrap();
    assert_eq!(
        stores.get("vault_a"),
        Some(&store(
            "vault_a",
            "minio+https://minio.example.com/vault-a",
            "/home/me/work/vault/arky.toml",
        ))
    );
    assert_eq!(
        stores.get("vault_b"),
        Some(&store("vault_b", "file:b", "/home/me/arky.toml"))
    );
    assert_eq!(stores.get("vault_d"), None);
}

#[test]
fn a_walk_from_higher_up_reads_only_the_files_above() {
    let stores = SavedStores::discover(&sample(), Path::new("/home/me/work")).unwrap();
    let urls: Vec<_> = stores.list().map(|saved| saved.url.as_str()).collect();
    assert_eq!(urls, ["file:a", "file:b", "s3://acme-c?region=eu-west-1"]);
}

#[test]
fn the_default_is_the_nearest_file_that_sets_it() {
    let files = system(&[
        ("/home/me/arky.toml", "default = \"far\"\n"),
        ("/home/me/work/arky.toml", "default = \"near\"\n"),
        (
            "/home/me/work/vault/arky.toml",
            "[lock]\nwait_seconds = 3\n",
        ),
    ]);
    let configuration = Configuration::discover(&files, Path::new("/home/me/work/vault")).unwrap();
    assert_eq!(configuration.default.as_deref(), Some("near"));
}

#[test]
fn a_default_no_file_defines_reads() {
    let configuration = discovered("default = \"nowhere\"\n").unwrap();
    assert_eq!(configuration.default.as_deref(), Some("nowhere"));
    assert_eq!(configuration.stores, SavedStores::default());
}

#[test]
fn each_lock_key_is_the_nearest_file_that_sets_it() {
    let files = system(&[
        (
            "/home/me/arky.toml",
            "[lock]\nexpires_seconds = 90\nwait_seconds = 7\n",
        ),
        ("/home/me/work/arky.toml", "[lock]\nexpires_seconds = 30\n"),
    ]);
    let configuration = Configuration::discover(&files, Path::new("/home/me/work")).unwrap();
    let both = LockSettings::new(Duration::from_secs(30), Duration::from_secs(7));
    assert_eq!(configuration.lock, both);

    let expires_only = discovered("[lock]\nexpires_seconds = 30\n").unwrap();
    let usual_wait = LockSettings::new(Duration::from_secs(30), LockSettings::default().wait);
    assert_eq!(expires_only.lock, usual_wait);

    let wait_only = discovered("[lock]\nwait_seconds = 7\n").unwrap();
    let usual_expires = LockSettings::new(LockSettings::default().expires, Duration::from_secs(7));
    assert_eq!(wait_only.lock, usual_expires);
}

#[test]
fn a_walk_with_no_file_anywhere_is_the_default() {
    let configuration = Configuration::discover(&MemorySystem::new(), Path::new("/home/me/work"));
    assert_eq!(configuration, Ok(Configuration::default()));
    let at_root = Configuration::discover(&MemorySystem::new(), Path::new("/"));
    assert_eq!(at_root, Ok(Configuration::default()));
}

#[test]
fn a_relative_folder_is_taken_against_the_current_folder() {
    let files = sample().with_current_dir("/home/me/work").unwrap();
    let configuration = Configuration::discover(&files, Path::new("vault")).unwrap();
    assert_eq!(configuration.lock.wait, Duration::from_secs(10));

    let above = Configuration::discover(&files, Path::new("vault/..")).unwrap();
    assert_eq!(above.lock.wait, Duration::from_secs(2));
    assert_eq!(
        above.stores.get("vault_a").map(|saved| saved.url.as_str()),
        Some("file:a")
    );
}

#[test]
fn other_top_level_keys_are_ignored() {
    let configuration = discovered("theme = \"dark\"\n\n[editor]\nwrap = true\n").unwrap();
    assert_eq!(configuration, Configuration::default());
}

#[test]
fn a_store_with_an_unknown_key_is_refused() {
    let text = "[stores.x]\nurl = \"file:a\"\nregion = \"x\"\n";
    assert_eq!(
        discovered(text),
        home_refused("[stores.x] has no key region; it takes url")
    );
}

#[test]
fn a_store_with_no_url_is_refused() {
    assert_eq!(
        discovered("[stores.x]\n"),
        home_refused("[stores.x] has no url")
    );
}

#[test]
fn a_store_whose_url_is_not_a_string_is_refused() {
    assert_eq!(
        discovered("[stores.x]\nurl = 5\n"),
        home_refused("[stores.x] url is not a string")
    );
}

#[test]
fn a_store_that_is_not_a_table_is_refused() {
    assert_eq!(
        discovered("stores = { x = \"file:a\" }\n"),
        home_refused("[stores.x] is not a table")
    );
    assert_eq!(
        discovered("stores = \"file:a\"\n"),
        home_refused("stores is not a table")
    );
}

#[test]
fn a_saved_store_that_names_another_is_refused() {
    let refusal = home_refused(
        "[stores.x] url names another saved store; a saved store's url is any store string but arky://",
    );
    assert_eq!(discovered("[stores.x]\nurl = \"arky://y\"\n"), refusal);
    assert_eq!(discovered("[stores.x]\nurl = \"ARKY://y\"\n"), refusal);
}

#[test]
fn a_default_that_is_not_a_string_is_refused() {
    assert_eq!(
        discovered("default = 3\n"),
        home_refused("default is not a string; it names a saved store")
    );
}

#[test]
fn a_lock_error_keeps_its_wording_after_the_file() {
    assert_eq!(
        discovered("[lock]\nexpires_seconds = 0\n"),
        home_refused("expires_seconds in [lock] is not a whole number of seconds above zero")
    );
    assert_eq!(
        discovered("[lock]\nwait_seconds = \"2\"\n"),
        home_refused("wait_seconds in [lock] is not a whole number of seconds above zero")
    );
    assert_eq!(
        discovered("[lock]\ntimeout = 5\n"),
        home_refused("[lock] has no key timeout; it takes expires_seconds and wait_seconds")
    );
    assert_eq!(
        discovered("lock = 5\n"),
        home_refused("lock is not a table")
    );
}

#[test]
fn a_file_that_does_not_read_is_refused() {
    assert_eq!(
        discovered("default = vault\n"),
        home_refused("`vault` is not a value (at byte 10)")
    );
}

#[test]
fn a_broken_file_farther_up_is_refused() {
    let files = system(&[
        ("/home/arky.toml", "[lock]\nwait_seconds = 0\n"),
        ("/home/me/work/arky.toml", WORK),
    ]);
    assert_eq!(
        Configuration::discover(&files, Path::new("/home/me/work")),
        Err(StoreError::NotConfigured(
            "/home/arky.toml: wait_seconds in [lock] is not a whole number of seconds above zero"
                .to_owned()
        ))
    );
}

#[test]
fn a_named_file_is_read_alone() {
    let files = sample()
        .with_file("/etc/arky.toml", WORK.as_bytes())
        .unwrap();
    let configuration = Configuration::read(&files, Path::new("/etc/arky.toml")).unwrap();
    let names: Vec<_> = configuration
        .stores
        .list()
        .map(|saved| saved.name.as_str())
        .collect();
    assert_eq!(names, ["vault_c"]);
    assert_eq!(
        configuration
            .stores
            .get("vault_c")
            .map(|saved| saved.defined_in.as_path()),
        Some(Path::new("/etc/arky.toml"))
    );
    assert_eq!(configuration.default, None);
    assert_eq!(configuration.lock, LockSettings::default());
}

#[test]
fn a_relative_named_file_is_taken_against_the_current_folder() {
    let files = sample().with_current_dir("/home/me").unwrap();
    let configuration = Configuration::read(&files, Path::new("work/vault/arky.toml")).unwrap();
    assert_eq!(
        configuration
            .stores
            .get("vault_a")
            .map(|saved| saved.defined_in.as_path()),
        Some(Path::new("/home/me/work/vault/arky.toml"))
    );

    let broken = files
        .with_file("/home/me/broken.toml", b"lock = 5\n")
        .unwrap();
    assert_eq!(
        Configuration::read(&broken, Path::new("broken.toml")),
        Err(StoreError::NotConfigured(
            "broken.toml: lock is not a table".to_owned()
        ))
    );
}

#[test]
fn a_named_file_that_is_missing_is_refused() {
    assert_eq!(
        Configuration::read(&sample(), Path::new("/etc/arky.toml")),
        Err(StoreError::NotConfigured(
            "/etc/arky.toml: the configuration file does not exist".to_owned()
        ))
    );
}
