# Move and merge with link integrity

Moving a folder, renaming a document, archiving something, and merging one folder
into another change the structure of a vault rather than the contents of one
document. arky treats them as one class of operation with one requirement:
every link that pointed into what moved still points at it afterwards, because
the operation rewrote those links as part of itself.

*The command spelling is not yet specified*, which is why this guide carries no
console blocks. See
[Structural operations](../reference/cli/structural-operations.md) for the shapes
it could take.

## What a move does

arky holds a vault as a structured graph, with typed resources and links as
first-class edges, so one operation:

1. moves the folder or the document;
2. rewrites every inbound link;
3. revalidates inheritance and the folder index files;
4. commits all of that or none of it.

The alternative — move, then detect what broke, then repair it — is what the
operation exists to replace. The difference is the one between an editor renaming
a symbol and a checker flagging dangling references afterwards.

## What a merge-move adds

A merge-move unions one folder into a destination that already exists and already
has contents, instead of replacing it. It:

- unions entry by entry into the existing destination;
- takes a collision policy chosen per invocation — fail, skip, or overwrite —
  defaulting to fail;
- removes the source folder once it has been emptied;
- rewrites every moved file's inbound links, because it composes with the move
  above;
- is atomic, so that if any single entry is refused, nothing moves at all.

## What gets rewritten

*Not yet specified.* Candidates for rewriting: paths anchored at a vault root,
relative paths, wiki-links, and embeds. Candidates for preserving: historical
spellings, commands quoted inside a document, and signed comment blocks whose
content must not change.

Region-aware rewriting already exists in `arky link`, which rewrites document
text without disturbing frontmatter, fenced blocks, inline code, existing links,
or raw web addresses. A move needs the same discrimination, resting on arky's
own parser. See [Link resources](link-resources.md).

## Why the validation rules are not a substitute

The rule for assets nothing references and the rule that a relation's target must
exist both report a break after it has happened. A structural operation cannot
break integrity in the first place. See
[Validation rules](../reference/validation-rules.md).

## How this fits the rest of arky

Every change arky makes produces a change object: a base plus a chain of
changes applied in order gives a byte-identical latest version, and unapplying
the chain in reverse returns the original. These operations are that model
applied to structure. See [Change objects](../reference/change-objects.md).

## Not yet specified

- The command spelling for each operation.
- Which reference spellings are rewritten and which are preserved.
- How the operation reports what it rewrote.
- What a dry run does.

See [Design status](../design-status.md) for the open questions here.

## Related

- [Structural operations](../reference/cli/structural-operations.md) — the
  command reference.
- [Changes and versions](../concepts/changes-and-versions.md) — the model these
  operations generalize into.
- [Link resources](link-resources.md) — the rewrite arky specifies in detail.
- [arky rm](../reference/cli/rm.md) — removal, which this class does not cover.
