# arky ls

`arky ls` lists the contents of a corpus.

## Synopsis

```console
$ arky ls
```

*Arguments and options are not yet specified.*

## Not yet specified

- What a row of the listing carries: a path alone, or a path together with the
  resource's type and other frontmatter values.
- Whether a listing recurses.
- Whether non-markdown files are listed alongside markdown ones.
- Whether the ignore list in `arky.toml` applies.
- What happens when no path is given.
- Output and exit codes.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [arky get](get.md) — retrieving what a listing names.
- [arky query](query.md) — selecting documents by an expression rather than by
  location.
- [arky and remargin](../../concepts/arky-and-remargin.md) — the overlap
  between arky's verbs and remargin's operations.
- [Command-line interface](index.md) — global options and the command table.
