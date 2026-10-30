# arky write

`arky write` stores a whole document at a path, with every file it embeds, read
from standard input or from a named file in one of four forms: markdown,
JavaScript Object Notation (JSON), binary, or zip. A second form of the command,
which writes into the part of one document that a path expression addresses, is
not yet specified; it is described under
[Writing into a part of a document](#writing-into-a-part-of-a-document).

## Writing a whole document

### Synopsis

```console
$ arky write <PATH> [--allow-missing] [-f, --format <FORMAT>] [--frontmatter <SYNTAX>] [-i, --input <FILE>] [-s, --store <STORE>]
```

The path is required.

### Arguments and options

| Part | What it takes |
|---|---|
| `<PATH>` | Where the document is stored. With no `--store`, a file of the machine, relative to the current folder or absolute. `.` and `..` are resolved from the text of the path, without following links, so `../notes/a.md` names the file a shell would name. With a `--store`, a path of that store, such as `notes/task.md`, used as written. |
| `--allow-missing` | Stores the document though a file it embeds is missing, or its embed leaves the document's folder, each such embed kept as written. |
| `-f`, `--format <FORMAT>` | The form the document is read in: `markdown`, the default, `json`, `binary`, or `zip`. |
| `--frontmatter <SYNTAX>` | The syntax the stored frontmatter is written in: `yaml`, the default, or `toml`. |
| `-i`, `--input <FILE>` | A file of the machine to read the document from, in place of standard input. It is never a path in a store. |
| `-s`, `--store <STORE>` | The store the document goes to: `minio:<bucket>`, `minio:<bucket>/<prefix>`, `s3:<bucket>`, `s3:<bucket>/<prefix>`, or `mongodb:<database>/<collection>`. The file system when it is left out. |

`arky --help` and `arky write --help` print help; `arky --version` prints
the version.

### The forms

| Form | What `arky write` reads |
|---|---|
| `markdown` from standard input | Markdown text, with YAML or TOML frontmatter. Each embed written as a data URI, as `arky get` writes markdown to standard output, becomes a file. |
| `markdown` from `--input FILE` | `FILE`, and each file it embeds, read at its path from `FILE`'s folder. A data URI in `FILE` becomes a file too. |
| `json` | The tree as one JSON value, as `arky get --format json` writes it. Each data URI becomes a file. |
| `binary` | The tree and each file it embeds in arky's binary layout, after a header of eight bytes: `arky`, a zero byte, and the version of the model's types, 4. The tree is read in place, as `arky get --format binary` writes it. |
| `zip` | A zip of `index.md` and each file it embeds at its path, as `arky get --format zip` writes it, or a zip whose one top folder holds them. |

Whatever the form, the file stored at the path is arky's form of the document:
the markdown arky writes for the tree it read, with the frontmatter in the syntax
`--frontmatter` names. Each file it embeds is stored beside it, at its path from
the document's folder: `media/logo.png`, embedded by `notes/report.md`, is stored
at `notes/media/logo.png`, and the stored text names it as `media/logo.png`, with
no data URI. A data URI's file goes to the path its `name` gives; one with no
`name` goes to `media/`, under the first 16 hexadecimal digits of the SHA-256 of
its bytes and an extension from its media type, such as
`media/4c4b6a3be1314ab8.png`.

### Embedded files

A document embeds a file with `![alt](target)` or `![[target]]` whose target is a
path from the document's folder, such as `![Logo](media/logo.png)`. A note it
embeds, such as `![[sub/note]]`, travels with the files that note embeds, found
from the note's folder. A link, `[text](target)`, carries no file.

A file the document embeds that the bytes do not hold, or that is not beside the
`--input` file, fails the command with code 1 and stores nothing:

```console
$ printf '![Logo](media/none.png)\n' | arky write notes/a.md
arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it
```

An embed whose target leaves the document's folder, such as `![Logo](../logo.png)`,
fails the same way: `index.md names ../logo.png, which leaves the bundle's folder`.
`--allow-missing` stores the document in both cases, each such embed kept as
written and no file stored for it.

### Behavior

`arky write` reads all of standard input, or all of the `--input` file and the
files it embeds, and then stores the document those bytes hold. A file already at
the path is replaced, as a copy replaces its target. A folder on the path that does
not exist is created.

The file is written whole: a temporary file beside it, then a rename over it, so a
reader sees the old file or the new one. Each file the document embeds is written
first, the same way, and the document last; no other file is touched but the
document's lock (see [Stores](#stores)). Bytes that do not hold a document in the
form store nothing.

`arky write` writes nothing to standard output. An error is one line on standard
error: `arky: `, the path as the command line gave it, and what failed.

```console
$ echo nope | arky write b.md --format json
arky: b.md: expected ident at line 1 column 2
```

### Stores

With no `--store`, the store is the file system.

With `--store minio:<bucket>` or `--store minio:<bucket>/<prefix>`, the store is
the bucket `<bucket>` on the MinIO server that the environment names, its files
kept as objects whose keys start with `<prefix>/`, or with no prefix when it is
left out: with the prefix `vault`, the store path `notes/task.md` is the object
`vault/notes/task.md`. The prefix follows the rules of a store path: parts joined
by `/`, none empty, none `.` or `..`. The bucket goes in the path of each
request's address, `<endpoint>/<bucket>/<key>`. A write creates the bucket when
it does not exist; the keys then need the right to create a bucket.

| Variable | What it holds |
|---|---|
| `ARKY_MINIO_ENDPOINT` | The address of the MinIO server, such as `http://127.0.0.1:9000`. Required, with no default. |
| `ARKY_MINIO_REGION` | The region the server expects, such as `us-east-1`. Required. |
| `AWS_ACCESS_KEY_ID` | The access key every request is signed with. Required. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request is signed with. Required. No output and no error shows it. |

A missing variable exits with code 3, on one line that names it. A prefix that is
not a store path exits with code 2. A server that cannot be reached, or that
refuses the keys, exits with code 4. A server that refuses to create a missing
bucket exits with code 4, on one line that names the store, such as
`arky: minio:notes/vault: …`.

```console
$ export ARKY_MINIO_ENDPOINT=http://127.0.0.1:9000 ARKY_MINIO_REGION=us-east-1
$ export AWS_ACCESS_KEY_ID=arky AWS_SECRET_ACCESS_KEY=arky-secret
$ arky get notes/task.md | arky write notes/task.md --store minio:notes/vault
$ unset AWS_SECRET_ACCESS_KEY; arky write notes/task.md -s minio:notes/vault < a.md; echo "exit $?"
arky: AWS_SECRET_ACCESS_KEY is not set: it holds the MinIO secret key
exit 3
```

With `--store s3:<bucket>` or `--store s3:<bucket>/<prefix>`, the store is the
AWS S3 bucket `<bucket>` in the region that `ARKY_S3_REGION` names, its files
kept as objects whose keys start with `<prefix>/`, or with no prefix when it is
left out: with the prefix `vault`, the store path `notes/task.md` is the object
`vault/notes/task.md`. The prefix follows the rules of a store path: parts joined
by `/`, none empty, none `.` or `..`. A write creates the bucket when it does not
exist: private, in that region, and encrypted by default with the KMS key that
`ARKY_S3_KMS_KEY_ARN` names when it is set. A bucket that exists is left as it
is. To create a missing bucket, the keys need the right to create it, and to set
its encryption when `ARKY_S3_KMS_KEY_ARN` is set.

| Variable | What it holds |
|---|---|
| `ARKY_S3_REGION` | The AWS region of the bucket, such as `eu-west-1`. Required, with no default. |
| `ARKY_S3_KMS_KEY_ARN` | The ARN of the KMS key a bucket that the write creates is encrypted with by default. Optional; with none, the write sets no encryption on the bucket it creates. |
| `AWS_ACCESS_KEY_ID` | The access key every request is signed with. Required. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request is signed with. Required. No output and no error shows it. |

A missing variable exits with code 3, on one line that names it, and so does an
`ARKY_S3_KMS_KEY_ARN` that is not UTF-8. A prefix that is not a store path exits
with code 2. AWS that cannot be reached, or that refuses the keys, exits with
code 4. AWS that refuses to create a missing bucket or to set its encryption
exits with code 4, on one line that names the store, such as
`arky: s3:acme-notes/vault: …`.

```console
$ export ARKY_S3_REGION=eu-west-1 AWS_ACCESS_KEY_ID=AKIA... AWS_SECRET_ACCESS_KEY=...
$ export ARKY_S3_KMS_KEY_ARN=arn:aws:kms:eu-west-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab
$ arky write notes/report.md -i draft/report.md -s s3:acme-notes/vault; echo "exit $?"
exit 0
$ unset ARKY_S3_REGION; arky write notes/task.md -s s3:acme-notes/vault < a.md; echo "exit $?"
arky: ARKY_S3_REGION is not set: it holds the AWS region of the S3 bucket
exit 3
```

In the file system, in a MinIO store and in an S3 store, a write holds a lock on
the document while it writes: the file `<file>.arky-lock` beside the document,
such as `notes/report.md.arky-lock` for `notes/report.md`, or the object at that
key. It takes the lock before the first file the document embeds and gives it
back after the document. A second write of the same document that finds the lock
waits 2 seconds, then exits with code 4 when the lock is still held, having
written nothing. A lock older than 5 seconds has expired, and the next write of
the document removes it. The two durations, 5 and 2 seconds, are the defaults of
`expires_seconds` and `wait_seconds` in the `[lock]` table of
[`arky.toml`](../config.md).

```console
$ arky write notes/report.md -i draft/report.md; echo "exit $?"
arky: notes/report.md: the file changed, or a file is already there
exit 4
```

With `--store mongodb:<database>/<collection>`, the store is the collection
`<collection>` of the database `<database>`, on the MongoDB server that the
environment variable `ARKY_MONGODB_URI` names. The command line holds names
only, never an address or a credential. The collection keeps each document, and
the MinIO bucket that `ARKY_MONGODB_BUCKET` names keeps each file a document
embeds as the object `<database>/<collection>/files/<sha256>`, where `<sha256>`
is the SHA-256 of the file's bytes in hexadecimal, so no two collections share
an object. A prefix goes before the key: with `notes/mongodb`, a file of the
collection `docs` of the database `work` is the object
`mongodb/work/docs/files/<sha256>` of the bucket `notes`. One record of the
collection `<collection>_files` names each object.

| Variable | What it holds |
|---|---|
| `ARKY_MONGODB_URI` | The connection string of the MongoDB server, `mongodb://` or `mongodb+srv://`, with the server and its credentials. |
| `ARKY_MONGODB_BUCKET` | The MinIO bucket of the store's files, `<bucket>` or `<bucket>/<prefix>`. |
| `ARKY_MINIO_ENDPOINT`, `ARKY_MINIO_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | The MinIO server and its keys, as for `--store minio:`. |

A missing variable exits with code 3, on one line that names it. An
`ARKY_MONGODB_BUCKET` that is not `<bucket>` or `<bucket>/<prefix>`, or whose
prefix is not a store path, exits with code 3 too. A server that cannot be
reached exits with code 4. A write of a document that embeds a file whose
deletion stopped halfway exits with code 4, naming
[`arky asset finish-deletions`](asset.md), which finishes that deletion. A write
creates the bucket `ARKY_MONGODB_BUCKET` names when it does not exist.

```console
$ export ARKY_MONGODB_URI=mongodb://127.0.0.1:27017 ARKY_MONGODB_BUCKET=notes/mongodb
$ export ARKY_MINIO_ENDPOINT=http://127.0.0.1:9000 ARKY_MINIO_REGION=us-east-1
$ export AWS_ACCESS_KEY_ID=arky AWS_SECRET_ACCESS_KEY=arky-secret
$ arky get notes/task.md | arky write notes/task.md --store mongodb:notes/files
$ unset ARKY_MONGODB_BUCKET; arky write notes/task.md -s mongodb:notes/files < a.md; echo "exit $?"
arky: ARKY_MONGODB_BUCKET is not set: it holds the MinIO bucket of a MongoDB store's files
exit 3
```

An arky built without its default features has none of the MinIO store, the S3
store and the MongoDB store, and a `--store minio:`, `--store s3:` or
`--store mongodb:` value exits with code 3 in it:

```console
$ arky write b.md --store minio:notes/drafts < a.md
arky: minio:notes/drafts: this arky is built without the MinIO store
$ arky write b.md --store s3:notes/drafts < a.md
arky: s3:notes/drafts: this arky is built without the S3 store
```

A `--store` value of any other shape is a usage error, code 2.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | The document could not be read or written in the form: not UTF-8, not JSON, not this version of the binary form, or a value the frontmatter syntax cannot hold. Without `--allow-missing`, a file the document embeds that is missing, or an embed that leaves the document's folder. |
| 2 | The command line is wrong: an unknown option, a missing path, a value an option does not take, or a path that cannot name a file, such as `/`. |
| 3 | The store is not configured, or its configuration is wrong: a variable the store reads is not set or is not UTF-8, such as `ARKY_MONGODB_URI` or `ARKY_MONGODB_BUCKET` for `--store mongodb:`, `ARKY_MINIO_ENDPOINT` for `--store minio:`, or `ARKY_S3_REGION` or `ARKY_S3_KMS_KEY_ARN` for `--store s3:`, or the store is not built into this arky. |
| 4 | The store failed: a file system error, such as a folder that is not writable, a MongoDB or MinIO server or AWS that cannot be reached, a MinIO server or AWS that refuses the keys, or one that refuses to create a missing bucket. A failure to read standard input is code 4 too, and so is a write of a document whose lock another write of the same document holds, and a write to a MongoDB store that another write of the same document finished first, which the store refuses with `<path>: the file changed, or a file is already there`. |
| 6 | No file at the path of `--input`. |

Code 5 belongs to the language-model provider error of the validation run; see
[Exit codes](../exit-codes.md).

### Examples

The copy: [`arky get`](get.md) piped into `arky write` stores arky's form of
the source at the target, in each form when `--format` is the same on both sides,
and a second copy changes nothing.

```console
$ arky get notes/task.md | arky write copy.md
$ arky get copy.md | arky write again.md && cmp copy.md again.md && echo same
same
$ arky get notes/task.md -f binary | arky write copy.md -f binary
```

The same copy through a JSON file:

```console
$ arky get notes/task.md --format json --output task.json
$ arky write again.md -f json -i task.json
```

A document that embeds `media/logo.png`, stored from a file with its picture
beside it, from markdown whose picture is a data URI, and from a zip:

```console
$ ls draft draft/media
draft:
media  report.md

draft/media:
logo.png
$ arky write notes/report.md -i draft/report.md
$ arky get notes/report.md | arky write copy/report.md
$ ls copy/media
logo.png
$ arky get notes/report.md -f zip -o report.zip
$ arky write zipped/report.md -f zip -i report.zip
```

A document whose picture is missing, refused and then stored with its embed as
written:

```console
$ printf '![Logo](media/none.png)\n' | arky write notes/a.md; echo "exit $?"
arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it
exit 1
$ printf '![Logo](media/none.png)\n' | arky write notes/a.md --allow-missing; echo "exit $?"
exit 0
```

## Writing into a part of a document

*This form is not yet specified, and arky does not have it yet: `--path`,
`--append`, and `--from-body` are usage errors, code 2.*

### Synopsis

```console
$ arky write folder/file.md --path='# ##["Specs"][2]' --append --from-body='path-to-file.txt'
```

### Arguments and options

| Part | What it takes |
|---|---|
| `folder/file.md` | The document to write into. One file. |
| `--path=<expr>` | A path expression addressing the part of the document to write into. |
| `--append` | Append to the addressed part rather than replacing it. |
| `--from-body=<file>` | Take the content to write from the named file. |

### The path expression

A path expression combines a heading level written as a run of `#` characters, a
heading title written as `["Specs"]`, and an index written as `[2]`. The example
above selects the level-one heading, then the level-two heading titled "Specs"
beneath it, then the second child block under that heading.

Addressing a part of a document is what makes two other things possible. It is
what lets the query language reach inside a document instead of stopping at the
document boundary. And it is the unit remargin's permission system grants on:
view or edit access is given on files and on parts of markdown documents, and a
reader sees only the accessible portion. A shape can be declared for a whole
document or for whatever a path addresses, so the same notation bounds what a
schema governs.

### Behavior

Every write into a part of a document is a [change object](../change-objects.md):
a base document plus a chain of changes applied in order reproduces the latest
version byte for byte, and unapplying the chain in reverse returns the original.
The guarantee holds because the content is linted markdown with a known shape.

arky preserves the formatting of frontmatter it did not write. remargin's write
path emits arky's canonical form instead.

### Not yet specified

- What else a path may address: list items, paragraphs, tables, code fences,
  remargin comment blocks, and frontmatter keys.
- What `[2]` counts — the second child element under that heading, the second
  heading carrying that title, or something else.
- Whether the path expression and the query expression are one language or two.
- The mutation verbs beyond appending. `append`, `prepend`, `set`, `remove`, and
  `replace` are the candidates on record.
- Whether a set of documents may be given rather than one file, and whether
  content may be given inline or read from standard input.
- Whether one query-driven mass modification is a single change object or one per
  document touched.
- Output, and the exit codes for a path expression that addresses nothing and for
  a document that does not match the shape a write assumes.

Field-editing commands — setting a field, removing a field, adding to an array,
removing from an array, and a bulk edit driven by a `--where` option — are not
arky's shape; the query language drives modification instead.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [arky get](get.md) — the document at a path, in one of the four forms.
- [Query grammar](../query-grammar.md) — the path expression in full.
- [arky query](query.md) — the language that also drives modification.
- [Change objects](../change-objects.md) — the transactional model every write
  into a part of a document produces.
- [Shape language](../shape-language.md) — shapes declared for a document or a
  sub-part of one.
- [Command-line interface](index.md) — global options and the command table.
