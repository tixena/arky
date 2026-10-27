# arky import

`arky import` pulls items in from a source outside a corpus and writes them
into it.

## Synopsis

```console
$ arky import <SOURCE>
$ arky import google-tasks
```

`<SOURCE>` is the external source to import from and is required.
`google-tasks` is the one source worked out.

## Options

| Option | Effect |
|---|---|
| `--list <ID>` | A specific task list to import, named by its identifier. |
| `--dry-run` | Show what would be imported without writing anything. |
| `--target <PATH>` | Where imported items are written. Default: the inbox folder. |

## Behavior

Items are pulled from the source and written into the target folder, by default
the inbox — so an import leaves work for [`arky inbox`](inbox.md) to classify
and file rather than filing it directly.

## Examples

```console
$ arky import google-tasks
$ arky import google-tasks --dry-run --target inbox/
```

## Not yet specified

- What an imported item looks like once written: whether it carries frontmatter,
  which properties, and how a source item maps onto a file name.
- How authentication against Google Tasks is handled: whether a separate command
  signs in, and where the credentials are stored.
- Which sources beyond Google Tasks are supported.
- Output, and the exit codes for an authentication failure and an unreachable
  source.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [arky inbox](inbox.md) — what processes the items an import leaves behind.
- [Configuration](../config.md) — the inbox path that supplies the default
  target.
- [Command-line interface](index.md) — global options and the command table.
