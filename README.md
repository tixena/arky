# arky

arky reads a markdown file, frontmatter included, into a typed tree and writes the tree back as markdown. Code can read, change or build that tree in between. The same tree also travels as JSON, as a zip or a single markdown file that holds the images it embeds, and as a binary archive, and a document and its files can be kept in a folder, an S3-compatible bucket or MongoDB.

| Crate | What it does | Features |
|---|---|---|
| [`arky`](https://docs.rs/arky) | Parses, edits and writes documents: the frontmatter as typed values, the body as a tree of sections, blocks and inline nodes. Gives the markdown, JSON, zip and binary forms, and the `TypeScript`, Zod and JSON Schema descriptions of its types. | `zod` and `jsonschema` (default), `mongodb`, `binary-format` |
| [`arky-store`](https://docs.rs/arky-store) | Keeps documents and the files they embed in a folder, a bucket or MongoDB, behind one async `Store` trait. A save made from a stale read fails instead of overwriting. | `minio`, `s3`, `mongodb`, `binary-format`; none by default |
| `arky-cli` | The `arky` command: gets and writes documents in every form, from the disk or a store. | `binary-format`, `mongodb`, `minio` and `s3`, all on by default |

## Installation

```toml
[dependencies]
arky = "0.1"
arky-store = "0.1"
```

```console
$ cargo install arky-cli
```

## Parse, edit and write a document

Parse a file, read a frontmatter value, change it, and write the file back:

```rust
use arky::parser::ast::{RawDocument, Value};
use arky::parser::parse;
use arky::writer::{WriteOptions, write};

fn main() -> Result<(), arky::writer::WriteError> {
    let source = "---\ntitle: Bread\nservings: 2\n---\n# Bread\n\nMix, rest and bake.\n";
    let mut parsed = parse(source);
    assert!(parsed.dropped.is_empty());

    let RawDocument::Valid(document) = &mut parsed.document else {
        panic!("every frontmatter entry reads");
    };
    let frontmatter = document.frontmatter.as_mut().expect("the file has a frontmatter");
    assert_eq!(frontmatter.get("servings"), Some(&Value::Integer(2)));
    frontmatter.insert("servings".to_owned(), Value::Integer(4));

    let text = write(&parsed.document, &WriteOptions::default())?;
    assert_eq!(text, "---\ntitle: Bread\nservings: 4\n---\n# Bread\n\nMix, rest and bake.\n");
    Ok(())
}
```

`parse` returns no error. A frontmatter entry that does not read is left out and listed, with its byte span, in `Parsed::dropped`. A write gives arky's form of each construct (`#` headings, `-` bullets, `*em*`, backtick fences, padded pipe tables), so from the second write on the text does not change.

## Keep documents in a store

Every store implements `Store`. Each read gives a `Revision` and each write takes a `Precondition`, so a save made from a stale read fails with `StoreError::Conflict` and the other change stays:

```rust
use std::path::PathBuf;

use arky_store::file::FileStore;
use arky_store::{Precondition, Store as _, StoreError, StorePath};
use os_shim::mock::MemorySystem;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let system = MemorySystem::new();
    let store = FileStore::new(PathBuf::from("/notes"), &system);
    let path = StorePath::new("shopping.md")?;

    store.write(&path, b"- buy flour\n", &Precondition::Absent).await?;
    let read = store.read(&path).await?;
    store.write(&path, b"- buy flour\n- buy salt\n", &Precondition::None).await?;

    let stale = Precondition::Matches(read.revision);
    let saved = store.write(&path, b"- buy yeast\n", &stale).await;
    assert_eq!(saved, Err(StoreError::Conflict(path.clone())));
    assert_eq!(store.read(&path).await?.bytes, b"- buy flour\n- buy salt\n");
    Ok(())
}
```

| Store | What it keeps | Feature |
|---|---|---|
| `FileStore` | Each file at its path under a root folder, through an os-shim `System`: the disk, or `MemorySystem` in tests | none |
| `ObjectStore` | Each file as an object of one bucket, under an optional key prefix, on MinIO, another S3-compatible server, or AWS S3 | `minio` or `s3` |
| `MongoStore` | Each document as one MongoDB record whose frontmatter a query can name, and each embedded file once, by its SHA-256, in a MinIO bucket | `mongodb` |
| `AnyStore` | Whichever of the three a program opened at run time | none; each variant needs its store's feature |

`document::put_bundle` and `document::get` store and load a document together with each file it embeds. In the file store, the MinIO store and the S3 store, that save holds a lock that expires, so a save that crashed does not block the next one.

## The command line

```console
$ printf 'Plan\n====\n\n* draft\n* review\n' > plan.md
$ arky get plan.md
# Plan

- draft
- review
$ arky get plan.md -f json > plan.json
$ arky write plan.md -f json -i plan.json
$ arky get report.md -f zip -o report.zip
$ arky write copy/report.md -f zip -i report.zip
$ arky get report.md -o out/report.md --embed
$ arky get report.md -f binary | arky write archive/report.md -f binary
$ arky get post.md --frontmatter toml
$ arky fix notes
$ arky get plan.md -s file:notes
$ arky get arky://notes/plan.md
$ arky store list
```

A path with no store is a file on disk. `-s` names a store by a store string instead, such as `file:notes`, `minio+https://minio.example.com/notes/vault`, `s3://acme-notes/vault?region=eu-west-1` or `mongodb://db.example.com/work/docs?files=arky://media`, with `${NAME}` for a secret held in a variable, or by `arky://<name>`, a store saved as `[stores.<name>]` in an `arky.toml` of the current folder or a folder above it; the path itself may be a store string, and `ARKY_STORE` or the `default` of `arky.toml` names the store when neither does. `arky store list` lists the saved stores in use. `arky fix` removes the expired locks that stopped saves left behind.

## Documentation

- [arky on docs.rs](https://docs.rs/arky): the tree, the writer, the forms, extensions, and the limits of the written form.
- [arky-store on docs.rs](https://docs.rs/arky-store): the stores, preconditions, locks, and the MongoDB records.
- [`GRAMMAR.md`](https://github.com/tixena/arky/blob/master/crates/arky/GRAMMAR.md): the grammar the parser follows and the form each construct is written in.

## Tests

| Recipe | Runs |
|---|---|
| `just all` | lint, the tests, and the store tests |
| `just test-stores` | the store tests against MongoDB and RustFS servers in containers it starts and stops (needs docker) |
| `just test-s3` | the S3 store tests against a real AWS bucket; needs `ARKY_TEST_S3_REGION`, `ARKY_TEST_S3_BUCKET`, `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` |

## License

MIT
