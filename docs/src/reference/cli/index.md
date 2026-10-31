# Command-line interface

arky is one program, invoked as `arky <COMMAND>`.

```console
$ arky <COMMAND>
```

## Global options

These four options belong to the program rather than to any one command.

| Option | Effect |
|---|---|
| `-c`, `--config <FILE>` | The configuration file, read alone, in place of `ARKY_CONFIG` and of the `arky.toml` files from the current folder up. Accepted before and after the command name. |
| `-v`, `--verbose` | Increase verbosity. |
| `-q`, `--quiet` | Suppress non-error output. |
| `--version` | Print the version. |

## Configuration and environment

arky reads every `arky.toml` from the current folder up to the root of the file
system, a setting of a nearer file winning over the same setting of a farther
one; the settings and the rules of that walk are documented on the
[Configuration](../config.md) page. Configuration sources take precedence in this
order, highest first:

1. Options given on the command line.
2. Environment variables.
3. The `arky.toml` files.
4. Built-in defaults.

`--config` names one file, read alone in place of the walk, and wins over
`ARKY_CONFIG`, which names one file when `--config` names none. Every command reads
the configuration when it starts. A file that `--config` or `ARKY_CONFIG` names
and that does not exist, or an `arky.toml` that does not read, exits with code 3
and one line that names the file; a folder with no `arky.toml` adds nothing.

Seven environment variables are read, and each variable a
[store string](#store-strings) names as `${NAME}`:

| Variable | Effect |
|---|---|
| `ARKY_CONFIG` | The one configuration file read, in place of the `arky.toml` files from the current folder up, when `--config` names none. |
| `ARKY_STORE` | The store `arky get` and `arky write` use when their path is not a store string and `--store` names none: a store string, `arky://<name>` or a folder. An error of that store names `ARKY_STORE`, never its value. |
| `ANTHROPIC_API_KEY` | Credential for the language-model provider used by `arky inbox`. |
| `OPENAI_API_KEY` | Credential for the language-model provider used by `arky inbox`. |
| `NO_COLOR` | Turns colored output off. |
| `AWS_ACCESS_KEY_ID` | The access key every request of a MinIO store is signed with, when its store string carries no keys. |
| `AWS_SECRET_ACCESS_KEY` | The secret key every request of a MinIO store is signed with, when its store string carries no keys. No output and no error shows it. |

## Store strings

A store string names a store in one text shaped like a URL, such as
`minio+https://minio.example.com/notes/vault`. [`arky get`](get.md),
[`arky write`](write.md), [`arky fix`](fix.md) and
[`arky asset finish-deletions`](asset.md) take one in `--store`, `arky get` and
`arky write` take one in their path and in `ARKY_STORE`, and a saved store of
[`arky.toml`](../config.md) holds one in its `url`.

### The parts of a store string

```text
minio+https://arky:secret@minio.example.com:9000/notes/vault?region=us-east-1
```

| Part | In the sample | What it is |
|---|---|---|
| backend | `minio` | The kind of store: `file`, `minio`, `mongodb` or `arky`. |
| transport | `+https` | How the backend reaches its server: `+http` or `+https` after `minio`, `+srv` after `mongodb` for a server found by its DNS record. The other backends take none. |
| credentials | `arky:secret@` | The keys of a MinIO store, or the user and password of a MongoDB server, percent-encoded. Optional. |
| host | `minio.example.com` | The server. A MongoDB string may list several hosts, `h1:27017,h2:27017`. |
| port | `:9000` | The server's port. Optional. |
| container | `notes` | What holds the store: the bucket of a MinIO string, `<database>/<collection>` of a MongoDB string, the name of an `arky://` string. A `file:` string names a folder. |
| location | `vault` | A folder inside the container: the prefix of every object key in a bucket, or a folder inside a MongoDB collection or a saved store. Optional. |
| options | `?region=us-east-1` | `<option>=<value>` pairs, joined by `&`. Each backend takes its own, and refuses with code 2 an option it does not take. |

### The backends

| Backend | Form | Options |
|---|---|---|
| file | `file:///<absolute path>`, `file:<relative path>`, or a plain path | none |
| MinIO | `minio+https://[<key>:<secret>@]<host>[:<port>]/<bucket>[/<prefix>]`, or `minio+http://` | `region`, `us-east-1` when it is left out |
| MongoDB | `mongodb://[<user>:<password>@]<host>[:<port>][,<host>[:<port>]…]/<database>/<collection>[/<folder>]`, or `mongodb+srv://` | `files`, required: the MinIO store of the collection's files; every other option goes to the MongoDB driver as written |
| saved store | `arky://<name>[/<folder>]` | none |

One sample of each:

```text
file:notes
file:///srv/notes
minio+https://minio.example.com/notes/vault?region=us-east-1
mongodb+srv://${MONGO_USER}:${MONGO_PASSWORD}@cluster0.example.net/work/docs?files=arky://media
arky://notes
```

A `file:` string is read as written, never as a URL: `file:///srv/notes` is the
absolute folder `/srv/notes`, and `file:notes` and `file:./notes` are `notes`
under the current folder, or under the folder of the `arky.toml` that saves it.
It takes no option.

A MinIO string with no `<key>:<secret>@` signs its requests with
`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`. A prefix, and a folder after a
collection or a saved store's name, follow the rules of a store path: parts
joined by `/`, none empty, none `.` or `..`.

A MongoDB collection keeps each document, and the MinIO store its `files` option
names keeps each file a document embeds as the object
`<database>/<collection>/files/<sha256>`, after that store's prefix, where
`<sha256>` is the SHA-256 of the file's bytes in hexadecimal, so no two
collections share an object. One record of the collection `<collection>_files`
names each object. `arky://<name>` is the store saved as `[stores.<name>]` in
`arky.toml`.

A text whose scheme, the part before its first `:`, no backend answers is a plain
path, a folder of the file store: `notes`, `../notes` and `notes:report.md` name
what they name in a shell. `minio:` and `mongodb:` followed by anything but
`//` are refused with code 2 rather than read as folders, and a folder of such a
name is written with `./` in front:

```console
$ arky get a.md -s minio:notes; echo "exit $?"
arky: `minio:notes`: minio: is not a store string: a MinIO store is minio+http://<host>/<bucket>, and a folder of this name starts with ./
exit 2
$ arky get a.md -s ./minio:notes
# Folder
```

A `<scheme>://` text whose scheme no backend of this arky answers exits with code
3, and so does a MinIO or MongoDB string in an arky built without that
backend:

```console
$ arky get ideas.md -s 'hdfs://cluster/notes'; echo "exit $?"
arky: `hdfs://cluster/notes`: no backend for hdfs in this build
exit 3
```

### Variables

Before a backend reads a store string, each `${NAME}` in it is replaced by the
value of the environment variable `NAME`, percent-encoded, so a value that holds
`@`, `/` or `:` cannot change the string's structure: with `MINIO_PASSWORD` set to
`p@ss/word`, `minio+https://arky:${MINIO_PASSWORD}@minio.example.com/notes` signs
with the secret `p@ss/word`. In a plain path that names a store, in `--store`,
`ARKY_STORE` or a saved store's `url`, the value goes in as it is. The path of
`arky get` and `arky write` has its variables replaced only when it is a store
string. `$$` writes one `$`.

A variable that is not set, or is not UTF-8, exits with code 3 on one line that
names it and the store string as written:

```console
$ arky get ideas.md -s 'file:${VAULT}'; echo "exit $?"
arky: VAULT is not set: `file:${VAULT}` reads it
exit 3
$ VAULT='my notes' arky get ideas.md -s 'file:${VAULT}'
# Spaced
$ arky get a.md -s 'file:price$$list'
# Prices
```

No output and no error shows a variable's value. An error names the store string
as written, its variables unexpanded, and [`arky store list`](store.md) shows each
saved store's URL as written.

### A store string inside an option

A MongoDB store names the MinIO store of its files in its `files` option. Written
there inline, the MinIO store string is percent-encoded whole, as any value of an
option is, and a variable inside it is written `%24%7BNAME%7D`, the
percent-encoded `${NAME}`; the MongoDB backend replaces it when it reads the
nested string. The other way is to save the MinIO store under a name and write
`files=arky://<name>`, which needs no encoding by hand:

```text
mongodb://db.example.com/work/docs?files=minio%2Bhttps%3A%2F%2Farky%3A%24%7BMINIO_PASSWORD%7D%40minio.example.com%2Fmedia
mongodb://db.example.com/work/docs?files=arky://media
```

A `${` written as it is inside `files` exits with code 2:

```console
$ arky get ideas.md -s 'mongodb://db.example.com/work/docs?files=minio%2Bhttps%3A%2F%2Farky%3A${MINIO_PASSWORD}%40minio.example.com%2Fmedia'; echo "exit $?"
arky: `mongodb://db.example.com/work/docs?files=minio%2Bhttps%3A%2F%2Farky%3A${MINIO_PASSWORD}%40minio.example.com%2Fmedia`: a variable inside files is written %24%7BNAME%7D, or name that store arky://<name>
exit 2
```

### A store in the path, and in --store

In the path of `arky get` and `arky write`, a store string names the store and the
document's path in it. The store is the container, and the rest of the string is
the path:

| Path | Store | Path in the store |
|---|---|---|
| `arky://notes/ideas.md` | the saved store `notes` | `ideas.md` |
| `minio+https://minio.example.com/notes/vault/ideas.md` | the bucket `notes` | `vault/ideas.md` |
| `mongodb://db.example.com/work/docs/ideas.md?files=arky://media` | the collection `docs` of the database `work` | `ideas.md` |
| `file:notes/ideas.md` | the current folder | `notes/ideas.md` |
| `file:///srv/notes/ideas.md` | the folder `/` | `srv/notes/ideas.md` |

A `file:` path means what the same plain path means: `.` and `..` are resolved
from its text, so `file:../notes/ideas.md` reads `../notes/ideas.md` and
`file:///srv/../ideas.md` reads `/ideas.md`.

`--store` names only the store. A path written after the store in `--store` is a
folder inside it, and the path the command takes is the document's path in that
folder, used as written; `--store` never names the document. In a store, the
path is a store path, so `./ideas.md` exits with code 2:

```console
$ arky get file:notes/ideas.md
# Ideas at home
$ arky get ideas.md -s file:notes
# Ideas at home
$ arky get ideas.md -s notes
# Ideas at home
$ arky get plan.md -s notes/drafts
# Plan
$ arky get ./ideas.md -s notes; echo "exit $?"
arky: ./ideas.md: `./ideas.md` is not a store path
exit 2
```

### Which store a command uses

`arky get` and `arky write` use the first store given of:

1. The path, when it is a store string: `arky://<name>/<path>`, or any other
   store string.
2. `--store`.
3. `ARKY_STORE`.
4. The `default` of `arky.toml`, the saved store it names.
5. The file system, the path taken against the current folder.

In steps 2 to 4 the path is a path of that store, used as written. `arky fix` uses
`--store` or the file system, and `arky asset finish-deletions` requires
`--store`.

```console
$ cat ~/arky.toml
default = "notes"

[stores.notes]
url = "file:notes"
$ cd ~/work
$ cat arky.toml
[stores.notes]
url = "file:vault"
$ arky get ideas.md
# Ideas at work
$ ARKY_STORE=../notes arky get ideas.md
# Ideas at home
$ ARKY_STORE=../notes arky get ideas.md -s arky://notes
# Ideas at work
$ arky get arky://notes/ideas.md -s ../notes
# Ideas at work
```

### From the short forms

`minio:` and `mongodb:` followed by a bucket or a database named a store
before store strings, and environment variables gave the server, the region and
the rest. Each such store is now one store string, written in `--store` or saved
under a name:

```text
before                                       after
the server and its region in two variables   [stores.notes]
                                             url = "minio+http://127.0.0.1:9000/notes/vault?region=us-east-1"
arky get a.md -s minio:notes/vault           arky get a.md -s arky://notes
```

## Output formats

Three output formats exist: plain text, which is the default, JavaScript Object
Notation (JSON), and markdown. [`arky check`](check.md) selects among them with
`--format`, and the configuration file carries a default under its `[output]`
heading; each is shown in full on the [Output formats](../output-formats.md)
page. Three commands instead emit a single fixed report with no format choice —
the inheritance chain from [`arky trace`](trace.md), the interactive proposal
from [`arky inbox`](inbox.md), and the export document from
[`arky export`](export.md) — each shown on its own page. [`arky get`](get.md)
and [`arky write`](write.md) take `--format` with forms of their own instead:
`markdown`, the default, `json`, `binary`, or `zip`.

Each of those forms carries every file the document embeds, such as the picture
of `![Logo](media/logo.png)`. Markdown to or from a stream, and JSON, carry each
file inside its embed as a data URI; markdown to or from a file carries each file
beside it, at its path from the file's folder, unless `arky get --embed` asks for
the one file with every data URI in it; binary and zip carry the files inside
their bytes. Both commands refuse, with code 1, a document whose embedded file is
missing or whose embed leaves the document's folder, and `--allow-missing` keeps
such an embed as written:

```console
$ arky get notes/report.md -o out/report.md
$ arky get notes/report.md -o one.md --embed
$ arky get notes/report.md -f zip -o report.zip
$ arky write copy/report.md -i out/report.md
$ printf '![Logo](media/none.png)\n' | arky write notes/a.md --allow-missing
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success, no issues. |
| 1 | Validation errors found. |
| 2 | Warnings only, when `--fail-on warning` was given. |
| 3 | Configuration error. |
| 4 | File system error. |
| 5 | Language-model provider error. |

The table is written for the validation run; see
[Exit codes](../exit-codes.md).

`arky get` and `arky write` exit with their own codes, 0 to 4 and 6, given on
[their pages](get.md#exit-codes), `arky asset` with its own, 0 and 2 to 4,
given on [its page](asset.md#exit-codes), `arky fix` with its own, 0 and 2 to 4,
given on [its page](fix.md#exit-codes), and `arky store list` with its own, 0 and
2 to 4, given on [its page](store.md#exit-codes).

## The commands

| Command | What it does |
|---|---|
| [`arky ls`](ls.md) | Lists the contents of a corpus. |
| [`arky get`](get.md) | Writes one markdown document, with every file it embeds, to standard output or a file, in markdown, JSON, binary or zip. |
| [`arky query`](query.md) | Selects documents by an expression over their frontmatter, in a language that also drives modification. |
| [`arky search`](search.md) | Matches a pattern against document bodies and frontmatter. |
| [`arky write`](write.md) | Stores one whole markdown document, with every file it embeds, read from standard input or a file. |
| [`arky asset`](asset.md) | Commands on the files that documents embed: `arky asset finish-deletions` finishes each deletion of such a file that stopped halfway in a MongoDB store. |
| [`arky store`](store.md) | Commands on the saved stores of `arky.toml`: `arky store list` lists the saved stores in use from the current folder. |
| [`arky rm`](rm.md) | Removes a file or a folder. |
| [`arky link`](link.md) | Wraps mentions of a resource in links to that resource. |
| [`arky check`](check.md) | Validates a corpus, or one path within it. |
| [`arky init`](init.md) | Sets up a corpus: the configuration file and any missing folder index files. |
| [`arky fix`](fix.md) | Repairs the findings that need no human decision, and removes the locks saves left behind. |
| [`arky inbox`](inbox.md) | Classifies unfiled captures with a language model and proposes where each one goes. |
| [`arky trace`](trace.md) | Prints the chain of inherited properties for one file. |
| [`arky export`](export.md) | Serializes a corpus for another program to read. |
| [`arky import`](import.md) | Pulls items in from an external source. |
| [Structural operations](structural-operations.md) | Move, rename, archive, and merge as transactions that keep links valid. |

`arky help` prints help and has no page of its own.

arky also exposes a Model Context Protocol (MCP) server, documented in the
[Model Context Protocol server](../mcp/index.md) reference.

## Not yet specified

- What `--verbose` adds, whether repeating it increases verbosity further, and
  what happens when `--verbose` and `--quiet` are given together.
- Whether `--verbose` and `--quiet` are accepted before the command name, after
  it, or in both positions.
- Which language-model provider credential, `ANTHROPIC_API_KEY` or
  `OPENAI_API_KEY`, is consulted when both are set.
- Whether a path argument defaults to the configured corpus root for commands
  other than `check`, `fix`, `inbox`, and `export`; whether it may be a glob; and
  whether several paths may be given. None of this is open for `arky get` and
  `arky write`: each takes exactly one path, and requires it.
- Standard input and standard output as an interface for commands other than
  `arky get`, `arky write`, `arky asset finish-deletions`, `arky fix` and
  `arky store list`. Their pages specify them: `arky get` writes to standard
  output, `arky write` reads from standard input, `arky asset finish-deletions`
  writes one object key per line to standard output, `arky fix` writes the path
  of each lock it removed, one per line, and `arky store list` writes one saved
  store per line.
- Which exit codes apply to commands other than the validation run,
  `arky get`, `arky write`, `arky asset finish-deletions`, `arky fix` and
  `arky store list`. The codes of those five are on their pages.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [Configuration](../config.md) — the `arky.toml` file and its settings.
- [arky store](store.md) — the saved stores in use.
- [Output formats](../output-formats.md) — the three formats in full.
- [Exit codes](../exit-codes.md) — the code table.
- [Query grammar](../query-grammar.md) — the expression language shared by
  `arky query`, `arky search`, and `arky write`.
- [arky and remargin](../../concepts/arky-and-remargin.md) — where the
  boundary between the two runs.
