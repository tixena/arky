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
| `<PATH>` | The document to read. A [store string](index.md#store-strings), such as `arky://notes/task.md` or `file:notes/task.md`, names the store and the document's path in it. Otherwise, in the store that `--store`, `ARKY_STORE` or the `default` of `arky.toml` names, a path of that store, such as `notes/task.md`, used as written. With none of them, a file of the machine, relative to the current folder or absolute. `.` and `..` are resolved from the text of the path, without following links, so `../notes/a.md` names the file a shell would name. |
| `--allow-missing` | Gives the document though a file it embeds is missing, or its embed leaves the document's folder, each such embed kept as written. |
| `--embed` | Writes markdown to `--output` as one file, every embedded file inside it as a data URI. It needs `--output` and the markdown form. |
| `-f`, `--format <FORMAT>` | The form the document is written in: `markdown`, the default, `json`, `binary`, or `zip`. |
| `--frontmatter <SYNTAX>` | The syntax the frontmatter is written in, in the markdown and zip forms: `yaml`, the default, or `toml`. |
| `-o`, `--output <FILE>` | A file of the machine to write the document to, in place of standard output. It is never a path in a store. |
| `-s`, `--store <STORE>` | The store `PATH` is in: a store string, `arky://<name>` or a folder, in place of `ARKY_STORE`, the `default` of `arky.toml` and the file system. A path written after the store is a folder inside it; `--store` never names the document. A `PATH` that is a store string wins over it. |

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
gave it, and what failed. An error of a store names the store instead: its store
string in backticks, as written, or `ARKY_STORE` when that variable named it.

```console
$ arky get missing.md
arky: missing.md: no file at this path
```

## Stores

`arky get` reads the document from the first store given of: `PATH`, when it is a
store string; `--store`; `ARKY_STORE`; the `default` of `arky.toml`; and, with
none of them, the file system. [Store strings](index.md#store-strings) gives the
form of each store string, its variables, and this order in full.

A store string in `PATH` names the store and the document's path in it. A path
written after the store in `--store`, in `ARKY_STORE` or after a saved store's
name is a folder inside that store, and `PATH` is the document's path in that
folder, used as written: a store path, its parts joined by `/`, none empty, none
`.` or `..`. `--store` never names the document.

```console
$ arky get arky://notes/drafts/plan.md
# Plan
$ arky get drafts/plan.md -s arky://notes
# Plan
$ arky get plan.md -s arky://notes/drafts
# Plan
$ ARKY_STORE=arky://notes arky get drafts/plan.md
# Plan
$ arky get file:notes/drafts/plan.md
# Plan
```

In the file system, `file:notes` and the plain folder `notes` name the same
folder, and a `file:` path in `PATH` means what the same plain path means.

With `minio+https://<host>/<bucket>/<prefix>`, or `minio+http://`, the store is
the bucket `<bucket>` on that MinIO server, its files kept as objects whose keys
start with `<prefix>/`, or with no prefix when it is left out: with the prefix
`vault`, the store path `notes/task.md` is the object `vault/notes/task.md`. The
bucket goes in the path of each request's address, `<endpoint>/<bucket>/<key>`,
and must already exist: a get creates no bucket. The region is `us-east-1` unless
the option `region` names another. A string with no `<key>:<secret>@` signs every
request with `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. No output and no
error shows the secret.

A key that is not set exits with code 3, on one line that names it. A prefix that
is not a store path exits with code 2. A server that cannot be reached, or that
refuses the keys, exits with code 4.

```console
$ arky get notes/task.md -s minio+https://minio.example.com/notes/vault; echo "exit $?"
arky: `minio+https://minio.example.com/notes/vault`: AWS_ACCESS_KEY_ID is not set: it holds the MinIO access key
exit 3
```

This example needs a MinIO server, which the book's build does not have; it is
written from arky's code:

```console
$ export AWS_ACCESS_KEY_ID=arky AWS_SECRET_ACCESS_KEY=arky-secret
$ arky get notes/task.md | arky write notes/task.md -s minio+https://minio.example.com/notes/vault
$ arky get notes/task.md -s minio+https://minio.example.com/notes/vault > task.md
$ arky get notes/task.md | cmp - task.md && echo same
same
```

With `s3://<bucket>/<prefix>?region=<region>`, the store is the AWS S3 bucket
`<bucket>` in that region, its files kept as objects whose keys start with
`<prefix>/`, or with no prefix when it is left out, as in a MinIO store. The
bucket must already exist: a get creates no bucket. The option `region` is
required. A string with no `<key>:<secret>@` signs every request with
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. No output and no error shows the
secret.

A string with no region, or a prefix that is not a store path, exits with code 2.
A key that is not set exits with code 3, on one line that names it. AWS that
cannot be reached, or that refuses the keys, exits with code 4.

```console
$ arky get notes/task.md -s s3://acme-notes/vault; echo "exit $?"
arky: `s3://acme-notes/vault`: no region: s3 needs ?region=<region>
exit 2
$ arky get notes/task.md -s 's3://acme-notes/vault?region=eu-west-1'; echo "exit $?"
arky: `s3://acme-notes/vault?region=eu-west-1`: AWS_ACCESS_KEY_ID is not set: it holds the AWS access key
exit 3
```

This example needs an AWS S3 bucket, which the book's build does not have; it is
written from arky's code:

```console
$ export AWS_ACCESS_KEY_ID=AKIA... AWS_SECRET_ACCESS_KEY=...
$ arky get 's3://acme-notes/vault/notes/task.md?region=eu-west-1' > task.md
```

With `mongodb://<host>/<database>/<collection>?files=<MinIO store>`, or
`mongodb+srv://`, the store is the collection `<collection>` of the database
`<database>` on that MongoDB server. The collection keeps each document, and the
MinIO store that `files` names keeps each file a document embeds as the object
`<database>/<collection>/files/<sha256>`, after that store's prefix, where
`<sha256>` is the SHA-256 of the file's bytes in hexadecimal, so no two
collections share an object: with `files` naming the bucket `notes` and the prefix
`mongodb`, a file of the collection `docs` of the database `work` is the object
`mongodb/work/docs/files/<sha256>` of the bucket `notes`. One record of the
collection `<collection>_files` names each object. `files` is a MinIO store string
percent-encoded, or `arky://<name>`, as
[A store string inside an option](index.md#a-store-string-inside-an-option)
describes. Every other option goes to the MongoDB driver as written. The server
must be reachable when the store opens.

A string with no `files`, or whose `files` names a store other than MinIO, exits
with code 2. A key of the `files` store that is not set exits with code 3. A
server that cannot be reached exits with code 4.

```console
$ arky get notes/task.md -s mongodb://db.example.com/work/docs; echo "exit $?"
arky: `mongodb://db.example.com/work/docs`: files is required: the MinIO store of the collection's files
exit 2
```

This example needs a MongoDB server and a MinIO server, which the book's build
does not have; it is written from arky's code:

```console
$ cat arky.toml
[stores.media]
url = "minio+https://${MINIO_USER}:${MINIO_PASSWORD}@minio.example.com/notes/mongodb"

[stores.work]
url = "mongodb://${MONGO_USER}:${MONGO_PASSWORD}@db.example.com/work/docs?files=arky://media"
$ arky get notes/task.md | arky write notes/task.md -s arky://work
$ arky get arky://work/notes/task.md
```

An arky built without its default features has none of the MinIO, S3 and MongoDB
backends, and a store string of one of them exits with code 3 in it:

```console
$ arky get notes/task.md -s 's3://acme-notes/vault?region=eu-west-1'
arky: `s3://acme-notes/vault?region=eu-west-1`: no backend for s3 in this build
```

`minio:`, `s3:` and `mongodb:` followed by anything but `//`, the short forms that
named a store before store strings, exit with code 2; see
[Store strings](index.md#store-strings).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | The document could not be read or written in the form: not UTF-8, not JSON, not this version of the binary form, or a value the frontmatter syntax cannot hold. Without `--allow-missing`, a file the document embeds that is missing, or an embed that leaves the document's folder. |
| 2 | The command line is wrong: an unknown option, a missing path, a value an option does not take, `--embed` without `--output` or with a form other than markdown, or a path that cannot name a file, such as `/`, or that is not a path of its store, such as `./a.md` with a `--store`. A store string that does not parse, that names an option its backend does not take, or that is a short form, such as `minio:notes`, is code 2 too. |
| 3 | The store is not configured, or its configuration is wrong: a variable a store string names is not set or is not UTF-8, `AWS_ACCESS_KEY_ID` or `AWS_SECRET_ACCESS_KEY` is not set for a MinIO or S3 store string that carries no keys, `ARKY_STORE` or `ARKY_CONFIG` is not UTF-8, a saved store no `arky.toml` defines, a backend this arky is built without, a file that `--config` or `ARKY_CONFIG` names and that does not exist, or an `arky.toml` that does not read. |
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
