# Structural operations

Moving a folder, renaming a document, archiving something, and merging one folder
into another change the shape of a corpus rather than the contents of one
document. arky performs them as integral transactions that keep links valid by
construction, rather than validating and repairing afterwards.

*No command spelling is yet specified for any of them, so this page shows no
command line.*

## Behavior

arky holds a corpus as a structured graph, with typed resources and links as
first-class edges. A structural operation therefore moves the folder, rewrites
every inbound link, revalidates the inheritance and the index files, and does all
of it or none of it. This is a distinct class of operation from the validation
rules that detect broken references after the fact — the rule for assets nothing
references, and the rule that a relation's target must exist. It is a rename in
an editor rather than a checker that flags dangling references afterwards.

Not every reference is rewritten. Historical spellings, commands quoted inside a
document, and signed comment blocks whose content must not change are preserved.
Refusing to rewrite a protected region is the same discrimination
[`arky link`](link.md) makes when it skips frontmatter, fenced blocks, inline
code, existing links, and raw web addresses; both rest on arky's own parser
knowing what those regions are.

## Merge-move

A merge-move unions entry by entry into a destination that already exists and
already has contents. Its collision policy is chosen per invocation from failing,
skipping, and overwriting, and it defaults to failing. It removes the source
folder once that folder is emptied, and it rewrites every moved file's inbound
links, because it composes with the operation above. It is atomic: if any entry
is refused, nothing moves at all.

Forcing a plain move at a non-empty destination is not this operation. A force
that replaces destructively destroys what is already there; a merge-move unions.

## The transactional model

Every change, structural or not, produces a [change object](../change-objects.md)
modeled the way git operates: a base resource plus a chain of changes applied in
order yields a byte-identical latest version, and unapplying the chain in reverse
returns the original. The operations on this page are the case that generalizes
to every kind of change.

## Not yet specified

- The command spelling of each operation.
- Which reference spellings get rewritten. The candidates are paths anchored at
  a corpus root, relative paths, wiki-links, and embeds.
- How an operation reports what it rewrote.
- What a dry run does.
- Output and exit codes.

See [Design status](../../design-status.md) for the open questions around these
operations.

## Related

- [Move and merge with link integrity](../../guides/move-and-merge-with-links.md)
  — the how-to.
- [Change objects](../change-objects.md) — the transactional model these
  operations generalize into.
- [arky link](link.md) — the command that rewrites document text without
  touching protected regions.
- [arky rm](rm.md) — removal, which these operations do not cover.
- [Command-line interface](index.md) — global options and the command table.
