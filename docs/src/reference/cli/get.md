# arky get

`arky get` reads the document at a path, with every file it embeds, and writes
it to standard output, or to a named file, in one of four forms: markdown,
JavaScript Object Notation (JSON), binary, or zip.

## Synopsis

```console
$ arky get <PATH> [--allow-missing] [--embed] [-f, --format <FORMAT>] [--frontmatter <SYNTAX>] [-o, --output <FILE>] [-s, --store <STORE>]
```

The path is required.

## Arguments and options

| Part | What it takes |
|---|---|
| `<PATH>` | The document to read. With no `--store`, a file of the machine, relative to the current folder or absolute. `.` and `..` are resolved from the text of the path, without following links, so `../notes/a.md` names the file a shell would name. With a `--store`, a path of that store, such as `notes/task.md`, used as written. |
| `--allow-missing` | Gives the document though a file it embeds is missing, or its embed leaves the document's folder, each such embed kept as written. |
| `--embed` | Writes markdown to `--output` as one file, every embedded file inside it as a data URI. It needs `--output` and the markdown form. |
| `-f`, `--format <FORMAT>` | The form the document is written in: `markdown`, the default, `json`, `binary`, or `zip`. |
| `--frontmatter <SYNTAX>` | The syntax the frontmatter is written in, in the markdown and zip forms: `yaml`, the default, or `toml`. |
| `-o`, `--output <FILE>` | A file of the machine to write the document to, in place of standard output. It is never a path in a store. |
| `-s`, `--store <STORE>` | The store the document is in: `minio:<bucket>`, `minio:<bucket>/<prefix>`, `s3:<bucket>`, `s3:<bucket>/<prefix>`, or `mongodb:<database>/<collection>`. The file system when it is left out. |

`arky --help` and `arky get --help` print help; `arky --version` prints the
version.

## The forms

| Form | What `arky get` writes |
|---|---|
| `markdown` to standard output | arky's form of the document: the text arky writes for the tree it read, with the frontmatter in the syntax `--frontmatter` names. Each file the document embeds goes into its embed as a data URI that carries the file's path as `name`: `![Logo](data:image/png;name=media/logo.png;base64,…)`. |
| `markdown` to `--output FILE` | arky's form of the document in `FILE`, with no data URI, and each file it embeds at its path from `FILE`'s folder: `out/report.md` and `out/media/logo.png`. |
| `markdown` to `--output FILE`, with `--embed` | `FILE` alone, its text as for standard output. |
| `json` | The tree as one JSON value, followed by a newline, each file the document embeds written into its embed as a data URI, as for markdown to standard output. |
| `binary` | The tree and each file the document embeds in arky's binary layout, after a header of eight bytes: `arky`, a zero byte, and the version of the model's types, 4. |
| `zip` | A zip of `index.md`, arky's form of the document, and each file it embeds at its path from the document's folder. |

Standard output has no folder for the files, so markdown to standard output always
carries them as data URIs. `--embed` with any form other than markdown exits with
code 2.

[`arky write`](write.md) reads the same four forms, so a document that leaves
arky in a form comes back in it, with its files.

## Embedded files

A document embeds a file with `![alt](target)` or `![[target]]` whose target is a
path from the document's folder, such as `![Logo](media/logo.png)`. A note it
embeds, such as `![[sub/note]]`, travels with the files that note embeds, found
from the note's folder. A link, `[text](target)`, carries no file.

A file the document embeds that is not in the store fails the command with code 1,
naming the embed as written:

```console
$ arky get notes/a.md
arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it
```

An embed whose target leaves the document's folder, such as `![Logo](../logo.png)`,
fails the same way: `index.md names ../logo.png, which leaves the bundle's folder`.
`--allow-missing` gives the document in both cases, each such embed kept as
written and no file carried for it.

## Output

Standard output carries only the document's bytes. They go out unchanged, binary
included, whether standard output is a terminal or not.

Nothing is written to standard output or to the `--output` file until the bytes
are ready, so a failed `arky get` leaves no partial output. The `--output` file
is written whole: a temporary file beside it, then a rename over it. A folder on
its path that does not exist is created, and a file already there is replaced.
With markdown to `--output` and no `--embed`, each file the document embeds is
written first, the same way, and the `--output` file last; no other file is
touched.

An error is one line on standard error: `arky: `, the path as the command line
gave it, and what failed.

```console
$ arky get missing.md
arky: missing.md: no file at this path
```

## Stores

With no `--store`, the store is the file system.

With `--store minio:<bucket>` or `--store minio:<bucket>/<prefix>`, the store is
the bucket `<bucket>` on the MinIO server that the environment names, its files
kept as objects whose keys start with `<prefix>/`, or with no prefix when it is
left out: with the prefix `vault`, the store path `notes/task.md` is the object
`vault/notes/task.md`. The prefix follows the rules of a store path: parts joined
by `/`, none empty, none `.` or `..`. The bucket goes in the path of each
request's address, `<endpoint>/<bucket>/<key>`, and must already exist: a get
creates no bucket.

| Variable | What it holds |
|---|---|
| `ARKY_MINIO_ENDPOINT` | The address of the MinIO server, such as `http://127.0.0.1:9000`. Required, with no default. |
| `ARKY_MINIO_REGION` | The region the server expects, such as `us-east-1`. Required. |
| `AWS_ACCESS_KEY_ID` | The access key every request is signed with. Required. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request is signed with. Required. No output and no error shows it. |

A missing variable exits with code 3, on one line that names it. A prefix that is
not a store path exits with code 2. A server that cannot be reached, or that
refuses the keys, exits with code 4.

```console
$ export ARKY_MINIO_ENDPOINT=http://127.0.0.1:9000 ARKY_MINIO_REGION=us-east-1
$ export AWS_ACCESS_KEY_ID=arky AWS_SECRET_ACCESS_KEY=arky-secret
$ arky get notes/task.md | arky write notes/task.md --store minio:notes/vault
$ arky get notes/task.md --store minio:notes/vault > task.md
$ arky get notes/task.md | cmp - task.md && echo same
same
$ unset ARKY_MINIO_REGION; arky get notes/task.md -s minio:notes/vault; echo "exit $?"
arky: ARKY_MINIO_REGION is not set: it holds the region the MinIO server expects
exit 3
```

With `--store s3:<bucket>` or `--store s3:<bucket>/<prefix>`, the store is the
AWS S3 bucket `<bucket>` in the region that `ARKY_S3_REGION` names, its files
kept as objects whose keys start with `<prefix>/`, or with no prefix when it is
left out: with the prefix `vault`, the store path `notes/task.md` is the object
`vault/notes/task.md`. The prefix follows the rules of a store path: parts joined
by `/`, none empty, none `.` or `..`. The bucket must already exist: a get creates
no bucket.

| Variable | What it holds |
|---|---|
| `ARKY_S3_REGION` | The AWS region of the bucket, such as `eu-west-1`. Required, with no default. |
| `AWS_ACCESS_KEY_ID` | The access key every request is signed with. Required. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request is signed with. Required. No output and no error shows it. |

A missing variable exits with code 3, on one line that names it. A prefix that is
not a store path exits with code 2. AWS that cannot be reached, or that refuses
the keys, exits with code 4.

```console
$ export ARKY_S3_REGION=eu-west-1 AWS_ACCESS_KEY_ID=AKIA... AWS_SECRET_ACCESS_KEY=...
$ arky get notes/task.md | arky write notes/task.md --store s3:acme-notes/vault
$ arky get notes/task.md --store s3:acme-notes/vault > task.md
$ unset ARKY_S3_REGION; arky get notes/task.md -s s3:acme-notes/vault; echo "exit $?"
arky: ARKY_S3_REGION is not set: it holds the AWS region of the S3 bucket
exit 3
```

With `--store mongodb:<database>/<collection>`, the store is the collection
`<collection>` of the database `<database>`, on the MongoDB server that the
environment variable `ARKY_MONGODB_URI` names. The command line holds names
only, never an address or a credential. The collection keeps each document, and
the MinIO bucket that `ARKY_MONGODB_BUCKET` names keeps each file a document
embeds as the object `files/<sha256>`, where `<sha256>` is the SHA-256 of the
file's bytes in hexadecimal. A prefix goes before the key: with `notes/mongodb`,
the file is the object `mongodb/files/<sha256>` of the bucket `notes`. One record
of the collection `<collection>_files` names each object.

| Variable | What it holds |
|---|---|
| `ARKY_MONGODB_URI` | The connection string of the MongoDB server, `mongodb://` or `mongodb+srv://`, with the server and its credentials. |
| `ARKY_MONGODB_BUCKET` | The MinIO bucket of the store's files, `<bucket>` or `<bucket>/<prefix>`. |
| `ARKY_MINIO_ENDPOINT`, `ARKY_MINIO_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | The MinIO server and its keys, as for `--store minio:`. |

A missing variable exits with code 3, on one line that names it. An
`ARKY_MONGODB_BUCKET` that is not `<bucket>` or `<bucket>/<prefix>`, or whose
prefix is not a store path, exits with code 3 too. A server that cannot be
reached exits with code 4.

```console
$ export ARKY_MONGODB_URI=mongodb://127.0.0.1:27017 ARKY_MONGODB_BUCKET=notes/mongodb
$ export ARKY_MINIO_ENDPOINT=http://127.0.0.1:9000 ARKY_MINIO_REGION=us-east-1
$ export AWS_ACCESS_KEY_ID=arky AWS_SECRET_ACCESS_KEY=arky-secret
$ arky get notes/task.md | arky write notes/task.md --store mongodb:notes/files
$ arky get notes/task.md --store mongodb:notes/files
$ unset ARKY_MONGODB_BUCKET; arky get notes/task.md -s mongodb:notes/files; echo "exit $?"
arky: ARKY_MONGODB_BUCKET is not set: it holds the MinIO bucket of a MongoDB store's files
exit 3
```

An arky built without its default features has none of the MinIO store, the S3
store and the MongoDB store, and a `--store minio:`, `--store s3:` or
`--store mongodb:` value exits with code 3 in it:

```console
$ arky get notes/task.md --store minio:notes
arky: minio:notes: this arky is built without the MinIO store
$ arky get notes/task.md --store s3:notes
arky: s3:notes: this arky is built without the S3 store
```

A `--store` value of any other shape is a usage error, code 2.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | The document could not be read or written in the form: not UTF-8, not JSON, not this version of the binary form, or a value the frontmatter syntax cannot hold. Without `--allow-missing`, a file the document embeds that is missing, or an embed that leaves the document's folder. |
| 2 | The command line is wrong: an unknown option, a missing path, a value an option does not take, `--embed` without `--output` or with a form other than markdown, or a path that cannot name a file, such as `/`. |
| 3 | The store is not configured, or its configuration is wrong: a variable the store reads is not set or is not UTF-8, such as `ARKY_MONGODB_URI` or `ARKY_MONGODB_BUCKET` for `--store mongodb:`, `ARKY_MINIO_ENDPOINT` for `--store minio:`, or `ARKY_S3_REGION` for `--store s3:`, or the store is not built into this arky. |
| 4 | The store failed: a file system error, such as a folder that is not writable or a path that is a folder, a MongoDB or MinIO server or AWS that cannot be reached, or a MinIO server or AWS that refuses the keys. A failure to write standard output is code 4 too. |
| 6 | No file at the path. |

Code 5 belongs to the language-model provider error of the validation run; see
[Exit codes](../exit-codes.md).

## Examples

The copy: `arky get` piped into [`arky write`](write.md) stores arky's form
of the source at the target, and a second copy changes nothing.

```console
$ arky get notes/task.md | arky write copy.md
$ arky get copy.md | arky write again.md && cmp copy.md again.md && echo same
same
```

The same copy through a JSON file:

```console
$ arky get notes/task.md --format json --output task.json
$ arky write again.md -f json -i task.json
```

A document that embeds `media/logo.png`, in each form. To standard output, the
picture is a data URI in the text:

```console
$ arky get notes/report.md
# Report

![Logo](data:image/png;name=media/logo.png;base64,iVBORw0KGgo…)
```

To a file, the picture is a file beside it, unless `--embed` asks for one file:

```console
$ arky get notes/report.md -o out/report.md
$ ls out out/media
out:
media  report.md

out/media:
logo.png
$ arky get notes/report.md -o one/report.md --embed
$ ls one
report.md
```

In the binary form and in a zip, the picture is inside the bytes:

```console
$ arky get notes/report.md -f binary > report.bin
$ arky get notes/report.md -f zip -o report.zip
$ unzip -Z1 report.zip
media/
index.md
media/logo.png
```

`--embed` writes markdown only:

```console
$ arky get notes/report.md -f zip -o report.zip --embed; echo "exit $?"
arky: --embed writes markdown; it needs -f markdown
exit 2
```

A document whose picture is gone, refused and then given with its embed as
written:

```console
$ arky get notes/a.md; echo "exit $?"
arky: notes/a.md: index.md names media/none.png, and the bundle does not hold it
exit 1
$ arky get notes/a.md --allow-missing
![Logo](media/none.png)
```

A missing file:

```console
$ arky get missing.md; echo "exit $?"
arky: missing.md: no file at this path
exit 6
```

## Not yet specified

- What `arky get` returns for a folder, an image, or a document in Portable
  Document Format. Today a folder fails with code 4, and a file that is not UTF-8
  with code 1.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [arky write](write.md) — storing a document given in one of the four forms.
- [arky ls](ls.md) — listing what can be retrieved.
- [arky query](query.md) — selecting documents by an expression.
- [Documents and schemas](../../concepts/documents-and-schemas.md) — the document
  as an abstraction independent of a file.
- [Command-line interface](index.md) — global options and the command table.
