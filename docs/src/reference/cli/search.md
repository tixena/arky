# arky search

`arky search` matches a pattern against documents in a corpus.

## Synopsis

```console
$ arky search folder/ -gi "[0-9]?[a-z]*"
```

Takes a path to search under and a pattern to match. *The flags, and the rest of
the option set, are not yet specified.*

## Behavior

Search runs over document bodies, over frontmatter when frontmatter is specified,
and over both when clauses are given for both. Matching the two separately
requires knowing where frontmatter ends and the body begins, which is one of the
reasons arky carries its own parser.

Search is the same expression language as [`arky query`](query.md) with a
default stage that matches body text; frontmatter and body clauses combine the
way any other clauses do.

## Not yet specified

- How a frontmatter clause and a body clause are spelled on a command line.
- Whether the pattern is a regular expression or a literal string, and which
  dialect it is written in.
- What the flags in the synopsis mean.
- Whether the path may be omitted, repeated, or given as a glob.
- What a match is reported as — a file path, a line, a line number, a fragment,
  or the document — whether matches are counted, and whether the pattern is
  matched line by line or across a whole document.
- How a match inside a fenced code block or a remargin comment block is treated.
- Output, and whether finding no match is a success or a failure.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [arky query](query.md) — the expression language.
- [Query grammar](../query-grammar.md) — search as a default stage of that
  language.
- [Command-line interface](index.md) — global options and the command table.
