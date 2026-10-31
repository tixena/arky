# Exit codes

A validation run returns one of six exit codes.

| Code | Meaning |
|---|---|
| 0 | Success, no issues |
| 1 | Validation errors found |
| 2 | Warnings only, when the run is set to fail on warnings |
| 3 | Configuration error |
| 4 | File system error |
| 5 | Language-model provider error |

Exit codes together with [JSON output](output-formats.md) are what make arky
friendly to [continuous integration](../guides/run-in-ci.md).

## What decides between 0, 1, and 2

The validation command takes the level at which a run fails — error, warning, or
any, with error as the default. Code 1 is what a run returns when it found errors
and errors are fatal. Code 2 is what it returns when it found no errors, found
warnings, and warnings are fatal.

*What a run returns when the level is set to any and only information-level
findings were made, and whether findings below the configured level change the
code at all, are not yet specified.*

## The other three codes

Code 3 covers an error in the [configuration](config.md), code 4 a file system
error, and code 5 an error from the language-model provider. Code 5 belongs to
the classification of inbox captures; see [arky inbox](cli/inbox.md).

*What counts as each — whether a missing configuration file is a configuration
error or a file system error, for instance — is not yet specified.*

## get and write

[`arky get`](cli/get.md#exit-codes) and [`arky write`](cli/write.md#exit-codes)
return one of six exit codes of their own.

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | The document could not be read or written in the form: not UTF-8, not JSON, not this version of the binary form, or a value the frontmatter syntax cannot hold. Without `--allow-missing`, a file the document embeds that is missing, or an embed that leaves the document's folder. |
| 2 | The command line is wrong: an unknown option, a missing path, a value an option does not take, `--embed` without `--output` or with a form other than markdown, or a path that cannot name a file, such as `/`, or that is not a path of its store, such as `./a.md` with a `--store`. A store string that does not parse, that names an option its backend does not take, or that is a short form, such as `minio:notes`, is code 2 too. |
| 3 | The store is not configured, or its configuration is wrong: a variable a store string names is not set or is not UTF-8, `AWS_ACCESS_KEY_ID` or `AWS_SECRET_ACCESS_KEY` is not set for a MinIO store string that carries no keys, `ARKY_STORE` or `ARKY_CONFIG` is not UTF-8, a saved store no `arky.toml` defines, a backend this arky is built without, a file that `--config` or `ARKY_CONFIG` names and that does not exist, or an `arky.toml` that does not read. |
| 4 | The store failed: a file system error, such as a folder that is not writable or a path that is a folder, a MongoDB or MinIO server that cannot be reached, a MinIO server that refuses the keys, or one that refuses to create a missing bucket for `arky write`. A failure to write standard output is code 4 too. |
| 6 | No file at the path. |

For `arky write`, a failure to read standard input is code 4 too, and code 6 is
no file at the path of `--input`.

## What an error message says

An error message is clear about what is wrong, specific about where, and
actionable.

```console
$ arky check
ERROR: Invalid type 'meeting'
  → src/notes/standup.md:2

  The type 'meeting' is not recognized. Did you mean 'note'?
  Valid types: index, person, organization, project, task, ...

  Hint: Use 'type: note' with 'tags: [meeting]' instead.
```

Two things separate this from the same rule's message in the catalog, which names
only the file, the offending value, and the valid types: a line number, and a
suggested correction — a nearest known type, and a way to keep the rejected word
as a tag instead. See [Validation rules](validation-rules.md).

*Whether every rule offers a suggestion, and how a nearest known value is chosen,
are not yet specified.*

## Not yet specified

- Codes for the commands that read, write, and search rather than validate, other
  than `arky get` and `arky write`, whose codes are under
  [get and write](#get-and-write). The first table is written in the language of a
  validation run. See the [Command-line interface](cli/index.md).
- Whether a query that matched nothing returns success or a code of its own.
- What a refused transaction returns. A structural operation moves nothing at all
  if any entry is refused; see
  [Structural operations](cli/structural-operations.md).
- Whether a repair run has codes of its own, and whether a
  [link](cli/link.md) dry run with pending edits exits non-zero.

See [Design status](../design-status.md) for the open questions around exit
codes.
