# arky asset

`arky asset` holds the commands on the files that documents embed, such as the
picture of `![Logo](media/logo.png)`. Its one command is
`arky asset finish-deletions`.

## arky asset finish-deletions

`arky asset finish-deletions` finishes each deletion of an embedded file that
stopped halfway in a MongoDB store.

### Synopsis

```console
$ arky asset finish-deletions -s, --store <STORE>
```

The store is required.

### Options

| Option | What it takes |
|---|---|
| `-s`, `--store <STORE>` | The store: a [store string](index.md#store-strings) or `arky://<name>`. Only a MongoDB store keeps deletions. |

`arky asset --help` and `arky asset finish-deletions --help` print help.

### Behavior

A MongoDB store keeps each file a document embeds as the object
`<database>/<collection>/files/<sha256>` of its MinIO bucket, named by one record
of the collection `<collection>_files`, as the MongoDB part of
[arky write](write.md#stores) describes. When no document
embeds a file any longer, a write or a deletion of a document deletes the file in
three steps: it marks the file's record, deletes the object from the bucket, then
deletes the record. When the bucket or the MongoDB server fails after the mark, or
arky stops there, the record stays marked. Every later write of a document that
embeds the same bytes then fails with code 4, naming
`arky asset finish-deletions`.

`arky asset finish-deletions` finishes each of those deletions: for each marked
record, it deletes the object, an object already gone counting as deleted, then
the record. After it, a write of those bytes stores them again.

The store is opened as `arky write --store` opens it, from its store string,
`mongodb://<host>/<database>/<collection>?files=<MinIO store>`, or the saved
store `arky://<name>` names. `arky asset finish-deletions` reads neither
`ARKY_STORE` nor the `default` of `arky.toml`.

### When to run it

Run it only while no other arky writes to the store. A deletion that is only
slow looks the same as one that stopped. When this command removes the record of
a slow deletion and a write then uploads the same bytes again, the slow deletion
can still delete the new object, and the document that embeds it loses its file.

### Output

The path of each object whose deletion it finished, `files/<sha256>`, under
`<database>/<collection>/` of the bucket, one per line on standard output, sorted. Nothing when no deletion had stopped.

This example needs a MongoDB server and a MinIO server, which the book's build
does not have; it is written from arky's code:

```console
$ arky asset finish-deletions -s arky://work
files/078892d2cf4160e6c295d580dd3a200f6189e9c5104b398162985b9a9cd299d5
$ echo $?
0
```

An error is one line on standard error: `arky: `, the store as the command line
gave it, and what failed. A file store, a MinIO store and an S3 store keep no
deletions, so a `--store` that names one exits with code 2:

```console
$ arky asset finish-deletions -s file:notes
arky: `file:notes` keeps no deletions: asset finish-deletions takes a MongoDB store
$ arky asset finish-deletions -s arky://notes
arky: `arky://notes` keeps no deletions: asset finish-deletions takes a MongoDB store
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success: each deletion that stopped halfway is finished. |
| 2 | The command line is wrong: no `--store`; a `--store` that names a file, MinIO or S3 store, which keeps no deletions; or a store string that does not parse, that names an option its backend does not take, or that is a short form, such as `minio:notes`. |
| 3 | The store is not configured, or its configuration is wrong: a variable a store string names is not set or is not UTF-8, `AWS_ACCESS_KEY_ID` or `AWS_SECRET_ACCESS_KEY` not set for a `files` store string that carries no keys, a saved store no `arky.toml` defines, a backend this arky is built without, or an `arky.toml` that does not read. |
| 4 | The store failed: a MongoDB or MinIO server that cannot be reached, or a MinIO server that refuses the keys. A failure to write standard output is code 4 too. |

## Related

- [arky write](write.md) — the MongoDB store, and a write that meets a deletion
  that stopped halfway.
- [Command-line interface](index.md) — global options and the command table.
