# Link resources

`arky link` finds plain-text mentions of a resource across a folder and turns
them into links to that resource. This guide covers preparing the resources,
running the command, reading what it produced, and undoing it.

## Step 1: have the resources that supply the targets

Link targets come from the resources themselves. Each resource carries a name and
a list of aliases; every name and alias becomes a string a mention can match, and
the link target is a relative path from the file being edited to that resource's
document. What counts as a linkable resource is declared by remargin — see
[Enforce resource types](enforce-resource-types.md).

*Which frontmatter fields of a resource supply its matchable strings, and how a
target is chosen when the resource is a folder rather than a single file, are not
yet specified.*

## Step 2: run it over a folder

```console
$ arky link folder/
```

Every markdown file under `folder/` is scanned and every qualifying mention is
rewritten in place. *Options are not yet specified.*

## Step 3: read what it produced

- **Mentions become links.** Every collision-free mention is wrapped in a
  markdown link whose target is a relative path to the resource. The display text
  is the matched text, unchanged.
- **Matching is deterministic.** No heuristics and no scoring: the same resources
  and the same files produce the same edits.
- **The longest match wins,** on Unicode word boundaries.
- **Collisions are refused, not guessed.** A string claimed by two or more
  resources is never linked, and each such string is reported as excluded.
- **Every edit is reported** — the file, the byte range, the matched text, and
  the link target — together with the excluded strings.

## What a run never touches

Some regions are never rewritten: frontmatter, fenced code blocks — remargin
comment blocks among them, because those are fenced blocks — inline code spans,
existing markdown links and wiki-links, raw web addresses, and a resource's own
document.

Telling those regions apart is what arky's own parser is for; a structural move
needs the same discrimination when it rewrites inbound links.

## Step 4: undo the run

```console
$ arky unlink folder/
```

`arky unlink` strips exactly the links a link run produces: a link whose
display text is a collision-free mention and whose target is what `arky link`
would emit. A link it could not have produced is left alone.

## Why this is a change like any other

Linking is additive — a link only wraps text that was already present, so
stripping every injected link reproduces the file byte for byte — it is
idempotent, because text already inside a link is skipped, and its inverse is
exact. Those are the promises a [change object](../reference/change-objects.md)
makes, in the small.

## Not yet specified

- Which frontmatter fields supply a resource's matchable strings.
- The link target when a resource is a folder.
- A dry run, an edit report format, and a guard against running on a working tree
  with uncommitted changes.
- Output and exit codes.

See [Design status](../design-status.md) for the open questions here.

## Related

- [arky link](../reference/cli/link.md) — the command reference.
- [Change objects](../reference/change-objects.md) — the model this command
  follows in miniature.
- [Move and merge with link integrity](move-and-merge-with-links.md) — the other
  operation that rewrites references.
- [A first session with arky](../tutorial/first-session.md) — the same steps
  front to back.
