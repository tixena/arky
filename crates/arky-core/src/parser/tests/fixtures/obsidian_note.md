---
aliases:
  - Parser design
  - Tree design
tags: [arky, design/parser]
created: 2026-09-12
status: in-progress
cssclasses: []
---
# Parser design

Follows from [[arky]] and the discussion in [[2026-09-10 planning|the planning note]]. See also [[grammar#Body]].

> [!note] Open point
> The tree and the grammar change together.

## Decisions

- One tree for every consumer
  - frontmatter holds values, not syntax
  - spans are byte ranges
- TOML first, YAML also read

## Tasks

- [x] Model the tree
- [ ] Write the lexer #next
- [ ] Review with [[Ada Lovelace]]

![[tree-sketch.png]]

#design #arky
