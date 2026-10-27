# Change objects

A *change object* is arky's representation of one edit to a resource. Every
change produces one, and the model is the one git uses: a base resource plus a
chain of changes applied in sequence.

## The model

- **A base plus a chain.** A change is not a snapshot. The latest version is what
  the base becomes after the chain has been applied to it.
- **Byte-identical replay.** Applying the chain to the base reproduces the latest
  version exactly, down to the bytes.
- **Reversibility.** Unapplying the chain in reverse returns the original.

The model holds because the content is linted markdown with a known shape, so it
rests on the linter and on the [shape language](shape-language.md).

Every write through remargin is such a change: remargin's whole write path goes
through the model and no write bypasses it. The change model is arky's; the
write path is remargin's.

## Structural operations as transactions

Moving, renaming, archiving, and merging a folder are single transactions rather
than an edit followed by a repair: move the folder, rewrite every inbound link,
revalidate the indexes — atomically, or not at all. A merge-move unions per entry
into an existing destination under an explicit collision policy of fail, skip, or
overwrite, defaulting to fail; it removes the source folder once it is emptied,
and moves nothing at all if any entry is refused.

*Which spellings of a reference get rewritten — root-anchored paths, relative
paths, wiki-links, embeds — and which are preserved, how the operation reports
what it rewrote, and what a dry run does, are not yet specified.* See
[Structural operations](cli/structural-operations.md).

## Linking as the worked example

[arky link](cli/link.md) is the model applied to one kind of edit, in the
small. Linking is additive — a link only wraps text that is already present, so
stripping every injected link reproduces the file byte for byte — it is
idempotent, and `arky unlink` is its exact inverse, removing only a link that
`arky link` would have produced. Additive and exactly reversible is what a
change object has to be.

## Where versions live

Version tracking sits above a pluggable storage backend: a filesystem, git, an
object store, a document database. Nothing above it needs to know which. Some
capabilities are easier to provide on some backends than on others, and a backend
may report a capability as not implemented. See
[Backends and capabilities](../concepts/backends-and-capabilities.md).

## Not yet specified

- **The format.** What a change object contains, how it is serialized, and
  whether it records an edit as a patch over the text or as an operation over the
  parsed document tree.
- **The identity.** How a change object is named, how a chain is ordered, and how
  a base version is identified.
- **The storage.** Where change objects are kept, how long, and whether the chain
  lives beside the document, inside it, or in the backend.
- **The transaction boundary for a query-driven change.** Whether one mass
  modification is a single change object covering every document touched or one
  per document. See [Query grammar](query-grammar.md).
- **What happens when replay is not byte-identical** — when a document has been
  edited outside arky, for instance.

See [Design status](../design-status.md) for the open questions around changes.

## Related

- [Changes and versions](../concepts/changes-and-versions.md) — the concept page.
- [Structural operations](cli/structural-operations.md) — moves and merges as
  transactions.
- [Query grammar](query-grammar.md) — mutation, and where a transaction ends.
- [Backends and capabilities](../concepts/backends-and-capabilities.md) — where a
  version is kept.
