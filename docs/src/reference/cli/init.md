# arky init

`arky init` sets up a corpus: it creates arky's configuration file and the
folder index files that are missing.

## Synopsis

```console
$ arky init [PATH]
$ arky init .
```

`[PATH]` is where to initialize; it is optional.

## Options

| Option | Effect |
|---|---|
| `--config` | Create `arky.toml` if it is missing. |
| `--indexes` | Create the missing `index.md` files. |
| `--all` | Do both. |

## Behavior

Two jobs. The first writes `arky.toml` at the repository root, the file whose
settings the [Configuration](../config.md) page documents. The second creates an
`index.md` in every folder that has none, carrying the properties that folder
inherits from the folders above it — the same repair the `index-required`
validation rule offers.

An index file declares an `inherits:` block of properties applying to every file
in the folder and its subfolders, and an `allows:` block naming which resource
types may live there and whether subfolders are permitted. See
[Inheritance](../../concepts/inheritance.md).

## Examples

```console
$ arky init .
$ arky init --all
```

## Not yet specified

- The default path when `[PATH]` is omitted, and what the command does when none
  of the three options is given.
- Whether an existing configuration file is left alone or refreshed.
- What a newly created file contains beyond the inherited properties.
- Whether inheritance keeps the `inherits:` shape that `--indexes` writes.
- Output and exit codes.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [Configuration](../config.md) — the file this command creates.
- [Inheritance](../../concepts/inheritance.md) — what an index file declares.
- [arky check](check.md) — the `index-required` rule, which reports the folders
  this command fills in.
- [Command-line interface](index.md) — global options and the command table.
