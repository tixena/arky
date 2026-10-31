# Choose a backend

A *backend* is where the versions of a document are kept. arky tracks versions
above that store, so a caller works with documents and their history without
knowing whether the bytes land in a folder on a disk, in a git repository, or in
an object store.

## Know what the abstraction covers

The abstraction reaches far enough that a caller need not care which store sits
underneath. Stores of these kinds are in view, as examples rather than as a
supported list:

| Backend | What it is |
|---|---|
| A filesystem | A plain directory tree. |
| git | A version-control system that already keeps a history of its own. |
| Minio | An object store on hardware you run yourself. |
| MongoDB | A document database. |

## Select a backend

*Not yet specified.* No configuration key names a backend, and no operation of a
backend is defined. See [Configuration](../reference/config.md) for the keys that
do exist.

## Expect uneven support

Version tracking is far easier over some stores than over others, so a backend
may leave part of the interface unimplemented and still be a legitimate backend.
An operation available on one backend is therefore not guaranteed on the next:
write against the backend you configured, not against the abstraction as a
whole.

*How a caller learns that a backend cannot do something — by asking it beforehand
or by attempting the operation and reading the error — is not yet specified.*

## Do not confuse git's two roles

git is one candidate store. It is also the model the change chain is patterned
on: a base plus a sequence of changes that replays to a byte-identical latest
version. The change model does not require git as a store, and choosing git as a
store does not delegate the chain to git's own history. See
[Change objects](../reference/change-objects.md).

## Not yet specified

- Which operations a backend provides.
- Whether a backend stores documents, change objects, or both.
- Which operations, if any, every backend must provide.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Backends and capabilities](../concepts/backends-and-capabilities.md) — the
  concept page.
- [Changes and versions](../concepts/changes-and-versions.md) — what gets
  tracked.
- [Documents and schemas](../concepts/documents-and-schemas.md) — the abstraction
  this one builds on.
