# arky link

`arky link` wraps plain-text mentions of known resources in links to those
resources.

## Synopsis

```console
$ arky link folder/
```

Scans the markdown files under `folder/` and rewrites each qualifying mention
into a relative-path markdown link. The set of linkable resources, and the
fields that supply the strings to match, come from the resource types declared
by remargin. *Options are not yet specified.*

## Behavior

- Every collision-free mention of a resource's name or alias is wrapped in a
  link; the display text is the matched text, unchanged.
- Matching is deterministic: no heuristics. The longest match wins, on Unicode
  word boundaries.
- Linking is additive: a link only wraps text that is already present, so
  stripping every injected link reproduces the file byte for byte.
- Linking is idempotent: text already inside a link is skipped, and a second run
  changes nothing.
- A string claimed by two or more resources is never linked; each such string is
  reported as excluded.
- Some regions are never rewritten: frontmatter, fenced code blocks, inline
  code, existing links and wikilinks, raw web addresses, and a resource's own
  profile document.
- Every run reports each edit — file, byte range, matched text, link target —
  together with the excluded strings.

Because linking is additive and byte-for-byte reversible, a link run is an
ordinary [change](../change-objects.md): it can be replayed and unapplied like
any other write.

## Undoing a run

`arky unlink` removes exactly the links a link run produces — a link whose
display text is a collision-free mention and whose target is what `arky link`
would emit — and touches nothing else. A link it could not have produced is left
alone.

## Not yet specified

- Which frontmatter fields of a resource supply its matchable strings.
- The link target when a resource is a folder rather than a single file.
- A dry run, an edit report format, and safety guards for uncommitted changes.
- Output and exit codes.

See [Design status](../../design-status.md) for the open questions around this
command.

## Related

- [Link resources](../../guides/link-resources.md) — the how-to.
- [Change objects](../change-objects.md) — replay and unapply.
- [Shapes, schemas, and resource types](../../concepts/shapes-schemas-resource-types.md)
  — where resource types are declared.
- [Command-line interface](index.md) — global options and the command table.
