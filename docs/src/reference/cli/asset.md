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
| `-s`, `--store <STORE>` | The MongoDB store, `mongodb:<database>/<collection>`. |

`arky asset --help` and `arky asset finish-deletions --help` print help.

### Behavior

A MongoDB store keeps each file a document embeds as the object `files/<sha256>`
of its MinIO bucket, named by one record of the collection `<collection>_files`,
as the MongoDB part of [arky write](write.md#stores) describes. When no document
embeds a file any longer, a write or a deletion of a document deletes the file in
three steps: it marks the file's record, deletes the object from the bucket, then
deletes the record. When the bucket or the MongoDB server fails after the mark, or
arky stops there, the record stays marked. Every later write of a document that
embeds the same bytes then fails with code 4, naming
`arky asset finish-deletions`.

`arky asset finish-deletions` finishes each of those deletions: for each marked
record, it deletes the object, an object already gone counting as deleted, then
the record. After it, a write of those bytes stores them again.

The store is opened as `arky write --store mongodb:` opens it, from the same
variables: `ARKY_MONGODB_URI`, `ARKY_MONGODB_BUCKET`, `ARKY_MINIO_ENDPOINT`,
`ARKY_MINIO_REGION`, `AWS_ACCESS_KEY_ID`, and `AWS_SECRET_ACCESS_KEY`.

### When to run it

Run it only while no other arky writes to the store. A deletion that is only
slow looks the same as one that stopped. When this command removes the record of
a slow deletion and a write then uploads the same bytes again, the slow deletion
can still delete the new object, and the document that embeds it loses its file.

### Output

The key of each object whose deletion it finished, `files/<sha256>`, one per line
on standard output, sorted. Nothing when no deletion had stopped.

```console
$ arky asset finish-deletions --store mongodb:notes/files
files/078892d2cf4160e6c295d580dd3a200f6189e9c5104b398162985b9a9cd299d5
$ echo $?
0
```

An error is one line on standard error: `arky: `, the store as the command line
gave it, and what failed. A MinIO store keeps no deletions, so a `--store minio:`
value exits with code 2:

```console
$ arky asset finish-deletions --store minio:notes
arky: minio:notes keeps no deletions: asset finish-deletions takes --store mongodb:<database>/<collection>
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success: each deletion that stopped halfway is finished. |
| 2 | The command line is wrong: no `--store`, a `--store` value of any other shape, or a `--store minio:` value. |
| 3 | The store is not configured, or its configuration is wrong: a variable the store reads is not set or is not UTF-8, an `ARKY_MONGODB_BUCKET` that is not `<bucket>` or `<bucket>/<prefix>`, or the MongoDB store is not built into this arky. |
| 4 | The store failed: a MongoDB or MinIO server that cannot be reached, or a MinIO server that refuses the keys. A failure to write standard output is code 4 too. |

## Related

- [arky write](write.md) — the MongoDB store, and a write that meets a deletion
  that stopped halfway.
- [Command-line interface](index.md) — global options and the command table.
