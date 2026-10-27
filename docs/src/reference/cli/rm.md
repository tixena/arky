# arky rm

`arky rm` removes a file or a folder from a corpus.

## Synopsis

```console
$ arky rm folder/file.md
$ arky rm folder/
```

The path is required. *Options are not yet specified: no confirmation flag, no
dry run, no recursion flag, and no force flag.*

## Not yet specified

- Whether removing a folder removes its contents.
- Whether anything is asked before a removal.
- Whether a removal is a [change object](../change-objects.md), and so whether it
  can be unapplied to restore what it removed.
- What happens to documents elsewhere that link to what was removed: refuse the
  removal, remove and report the now-dangling links, or rewrite them.
  [Structural operations](structural-operations.md) answer the equivalent
  question for moves; removal is not among them.
- Output and exit codes.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [Structural operations](structural-operations.md) — move, rename, archive, and
  merge as transactions that keep links valid.
- [Change objects](../change-objects.md) — the transactional model a removal
  would have to fit.
- [arky write](write.md) — the command that changes a document's contents.
- [Command-line interface](index.md) — global options and the command table.
