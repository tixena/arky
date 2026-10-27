# Changes and versions

arky's model of editing is not "open a file, write it back". Every change is an
object: a value that describes what was done to a resource, that applies to
produce the next version of that resource, and that unapplies to produce the
previous one. A document's history is the base document plus the ordered chain of
those objects.

## Every change is an object

The model works the way a version-control history works. Given a base resource and
a chain of changes applied one after another, you obtain a byte-identical latest
version. Given the latest version and the same changes unapplied in reverse order,
you arrive back at the original.

That guarantee is reachable because the content is linted markdown with a known
shape. A change to arbitrary text can only be described as a text difference; a
change to a document whose structure is known can be described in terms of that
structure.

The change model applies to remargin's entire write path, and nothing bypasses it.

## Two promises

The model makes two separate promises, and they fail differently.

*Replay to a byte-identical latest version.* Applying the chain to the base
reproduces the current document exactly, byte for byte — not a document that means
the same thing or renders the same way. That is what makes the chain a faithful
record rather than a summary.

*Unapply back to the original.* Reversing the chain arrives at the document as it
was, so every change object carries enough to undo itself, not only enough to redo
itself.

*How a change object is represented, what its parts are, and how the two
guarantees are checked are not yet specified.* See
[Change objects](../reference/change-objects.md).

## Structural operations

Move, rename, merge, and archive are integral transactions rather than filesystem
operations followed by a repair pass. arky holds a corpus as a structured graph,
with typed resources and links as first-class edges, so a move rewrites every
inbound link, revalidates inheritance and folder indexes, and either completes or
does nothing.

The distinction is a third class of verb. A rule that checks and then repairs —
the orphaned-asset rule, or the rule that relation targets must exist — detects a
broken reference after the fact. An integral move cannot break the reference in
the first place.

*Which reference spellings a move rewrites — root-anchored paths, relative paths,
wiki-links, embeds — and which are preserved, how the operation reports what it
rewrote, and what a dry run does are not yet specified.*

### Merge-move

A merge-move unions entry by entry into a destination folder that already exists
and already has contents, instead of replacing it. It takes a collision policy per
invocation — fail, skip, or overwrite, defaulting to fail — it removes the source
directory once it is emptied, it rewrites the inbound links of every moved file by
composition with the integral move above, and it is atomic: if any single entry is
refused, nothing moves at all.

*A command spelling for these operations is not yet specified.* See
[Structural operations](../reference/cli/structural-operations.md).

## A worked instance: additive, reversible linking

Linking is the smallest operation that carries both promises. `arky link` wraps
every collision-free mention of a resource in a relative-path link to that
resource. It is additive, so stripping every injected link reproduces the file
byte for byte, and `arky unlink` is an exact inverse that strips only a link
`arky link` itself would have produced.

The same operation shows what a parser has to know for such an edit to be safe. It
never rewrites frontmatter, fenced code blocks — remargin comment blocks included
— inline code spans, existing links and wiki-links, or raw web addresses. Those
skip zones are the parser knowing what a link, a fence, a frontmatter block, and a
comment block are. See [arky link](../reference/cli/link.md).

## Not yet specified

- Where a transaction ends: when one query drives a modification across many
  documents, whether that is one change object covering every document touched or
  one change object per document.
- Which mutation verbs exist beyond appending, which
  [arky write](../reference/cli/write.md) carries today — whether the language
  also sets, removes, replaces, and moves.
- How a change object relates to storage: whether a
  [backend](backends-and-capabilities.md) stores documents, change objects, or
  both, and what a backend must provide for replay and unapply to work.

See [Design status](../design-status.md) for the open questions here.
