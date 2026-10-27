# arky export

`arky export` serializes a corpus as JavaScript Object Notation (JSON) for a
language-model agent or other tooling to read.

## Synopsis

```console
$ arky export [PATH]
```

`[PATH]` is the path to export; it is optional.

## Options

| Option | Effect |
|---|---|
| `--include-content` | Include the content of each file. Makes the output large. |
| `--only-metadata` | Export only frontmatter, without the structure. |

## Behavior

The export carries, for each file: its frontmatter; the observations in its body,
which are list items opening with a bracketed category — a fact, a decision, a
tip, a warning, a question, or a task — each with its text and any tags; and the
relations in its body, which are typed links to other resources. It also carries
a graph of those links, in which each resource is a node and each relation an
edge carrying its type.

## Example

```console
$ arky export src/work/ --only-metadata
```

The export document:
```console
$ arky export
{
  "files": [
    {
      "path": "src/work/projects/service.md",
      "frontmatter": { "type": "project", "area": "work" },
      "observations": [
        { "category": "fact", "content": "Built on Rust with Actix-web", "tags": [] },
        { "category": "decision", "content": "Using PostgreSQL over MongoDB", "tags": ["database"] }
      ],
      "relations": [
        { "type": "owned_by", "targets": ["Platform Team"] },
        { "type": "depends_on", "targets": ["Authentication Service"] }
      ]
    }
  ],
  "graph": {
    "nodes": ["service.md", "Platform Team.md", "Authentication Service.md"],
    "edges": [
      { "from": "service.md", "to": "Platform Team.md", "type": "owned_by" },
      { "from": "service.md", "to": "Authentication Service.md", "type": "depends_on" }
    ]
  }
}
```

A relation's targets are the names inside the wiki-links in the document body,
while the graph's nodes and edges are file names, so the export resolves a name
to a file.

## Not yet specified

- How a relation target written as a name resolves to a file.
- Whether the output goes to standard output or to a file, what happens when both
  options above are given, and whether an export can be filtered by type.
- Whether serializing a corpus is a command or an engine capability remargin
  reaches through, and whether arky builds the graph the `graph` object carries.
- Exit codes.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [Knowledge patterns](../../concepts/knowledge-patterns.md) — observations,
  relations, and the graph the export carries.
- [Output formats](../output-formats.md) — the validation run's formats, which
  are separate from this command.
- [Use arky from an agent](../../guides/use-from-an-agent.md) — the audience.
- [Command-line interface](index.md) — global options and the command table.
