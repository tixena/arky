# Backends and capabilities

A version of a document has to be kept somewhere. arky puts that "somewhere"
behind an abstraction, so the engine tracks versions without the caller knowing
whether the bytes end up in a folder on a disk, in a git repository, or in an
object store on someone else's servers.

## Version tracking is backend-agnostic

arky tracks versions in a way abstracted far enough that no caller has to worry
about whether the backend is a filesystem, git, Minio, MongoDB, or any other
store. Those four are examples rather than a supported list:
a filesystem is a plain directory tree; git is a version-control system that
already keeps its own history; Minio is an object store on hardware one runs
oneself; MongoDB is a document database.

The abstraction depends on the document abstraction above it: version tracking
over a pluggable backend only means something once a document can exist without a
file underneath it. See [Documents and schemas](documents-and-schemas.md) for that
abstraction and [Changes and versions](changes-and-versions.md) for the model of
what gets tracked.

git appears here in two independent roles, and they should not be confused: it is
one candidate storage backend, and separately it is the model the change chain is
patterned on — a base plus a sequence of changes that replays to a byte-identical
latest version. The change model does not require git as a backend, and using git
as a backend does not delegate the change chain to git's own history.

## Uneven support is allowed

The abstraction does not reduce every backend to the same set of operations. A
backend may leave part of the interface not implemented and remain a legitimate
backend, because some stores make version tracking far easier than others. That
exemption is what "capabilities" names: a backend is described by what it can do,
not required to do everything.

## Not yet specified

- The backend interface: which operations a backend provides.
- What a backend stores: documents, change objects, or both.
- How "not implemented" is surfaced: whether a caller asks a backend what it
  supports before calling, or learns it from an error returned by an attempted
  call.
- How a backend is chosen and configured.
- Which operations, if any, every backend must provide — and therefore where the
  exemption stops.

See [Design status](../design-status.md) for the open questions here, and
[Choose a backend](../guides/choose-a-backend.md) for the guide.
