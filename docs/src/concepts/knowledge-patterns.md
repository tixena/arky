# Knowledge patterns

Frontmatter describes a document as a whole. Knowledge patterns describe things
*inside* it: a fact worth extracting, a decision that was made, a typed connection
to another resource. They are ordinary markdown list lines that follow a
convention, so a reader sees a bulleted list and arky sees structured data.

## Observations

An observation is a categorized fact written as a list line: a bracketed category,
then the content, then an optional tag.

```markdown
- [fact] The project uses PostgreSQL 15
- [decision] We chose gRPC over REST for internal services
- [tip] Run migrations before deploying
- [warning] Never expose the admin endpoint publicly
- [question] Should we support multi-tenancy?
- [todo] Implement rate limiting
```

Eight categories are built in, and configuration extends the set: `fact` for
objective information, `decision` for a choice that was made, `tip` for advice,
`warning` for a caution, `question` for something still open, `todo` for an inline
action item that is not a task file, `insight` for a realization, and `reference`
for an external resource.

An observation is not a tag: tags exist for discovery, while observations are
structured facts inside one document that can be extracted and aggregated across a
corpus, handed to a language model as context, and queried. A `todo` observation
is not a task either — every task is its own document, and a note carrying several
independent checkbox items is reported so it can be split into task files.

## Relations

A relation is a typed connection between resources, written as a list line whose
first word is the relation type and whose targets are wiki-links. One line may
name more than one target.

```markdown
- depends_on [[Authentication Service]]
- blocks [[Mobile App Release]]
- owned_by [[Platform Team]]
- related_to [[API Design]] [[Security Review]]
- supersedes [[Old Proposal]]
```

Nine relation types are built in, each with an inverse, and configuration extends
the set: `depends_on` inverts to `dependency_of`, `blocks` to `blocked_by`,
`owned_by` to `owns`, `part_of` to `contains`, `supersedes` to `superseded_by`,
`references` to `referenced_by`, `implements` to `implemented_by`, `extends` to
`extended_by`, and `related_to` inverts to itself.

Both patterns may appear anywhere in a document. Grouping them under their own
headings ahead of the prose is a convention, not a rule.

## What arky checks

Three checks run over the patterns, each with a severity configuration can set to
off, informational, warning, or error.

- **Observation format.** An observation line must follow the bracketed-category
  form. A malformed line is reported with the file, the line number, what was
  found, and what was expected. Warning by default.
- **Relation target exists.** A relation's target must resolve to a file. A target
  that resolves to nothing is reported with the file, the line number, and the
  line itself. Warning by default.
- **Relation type is known.** A relation type should come from the built-in set or
  from the additions configuration declares. An unknown type is reported
  informationally, with the known types listed as alternatives.

Configuration carries the two extensible lists and the three severities in one
block, alongside a switch that turns the whole area off — see
[Configuration](../reference/config.md) and
[Validation rules](../reference/validation-rules.md).

## What arky extracts

`arky export` emits, for each file, its frontmatter, its observations with their
categories and tags, and its relations with their types and targets. Alongside the
per-file entries it emits a graph: a list of nodes, and a list of edges each
carrying a source, a target, and the relation type connecting them. See
[arky export](../reference/cli/export.md).

## Not yet specified

- How a wiki-link target is resolved to a file — by filename, by a title in
  frontmatter, by a path, or otherwise.
- Whether the pattern checks become shapes, stay as built-in rules, or fall away
  once documents are governed by
  [shapes and schemas](shapes-schemas-resource-types.md).

See [Design status](../design-status.md) for the open questions here.
