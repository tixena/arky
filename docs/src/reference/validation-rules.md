# Validation rules

A validation run checks the documents in a corpus against a catalog of rules.
Each rule reports at one level — error, warning, or information — prints a fixed
message, and is either repairable without a person deciding anything or not.

The catalog assumes a fixed model: frontmatter on every file but the inbox,
`type` and `created` as fixed requirements, and a fixed list of resource types.
Under user-declared [shapes](shape-language.md) each becomes a consequence of a
document's governing schema rather than a rule in its own right. *How the catalog
is re-derived from shapes is not yet specified.*

## The rules

| Rule | Category | Level | What it checks | Repairable |
|---|---|---|---|---|
| `frontmatter-required` | Frontmatter | Error | Every markdown file has frontmatter, except the inbox when raw inbox files are allowed | *Not yet specified.* |
| `frontmatter-valid` | Frontmatter | Error | The frontmatter parses | No |
| `type-required` | Properties | Error | The file carries a `type` property | No — a person decides |
| `type-valid` | Properties | Error | The `type` value is one of the defined resource types | No — a person decides |
| `created-required` | Properties | Error | The file carries a `created` date | Yes, from the file's modification time |
| `inheritance` | Properties | Error | The file carries the properties declared in the `inherits:` block of a parent `index.md` | Yes, by adding them |
| `index-required` | Structure | Error | Every folder that is not ignored has an `index.md` | Yes, by creating it with the inherited properties |
| `allowed-types` | Structure | Error | A file's type is permitted by the `allows.types` list in its folder's `index.md` | No |
| `archive-placement` | Archive | Error | A file marked `archived: true` lives under an `archive/` folder | Yes, by moving the file |
| `orphaned-assets` | Assets | Warning | Every asset is referenced by at least one markdown file | No |
| `embedded-tasks` | Tasks | Warning | A note does not hold several independent checkbox items | No |
| `inbox-raw` | Inbox | Information | Names inbox files that still have no frontmatter | No — the inbox command processes them |

Rule settings — the accepted resource types, the archive property and folder, the
inbox path, the asset extensions, and the checkbox count that raises the
embedded-tasks warning — live in the [configuration file](config.md).

## The messages

### `frontmatter-required`

```
ERROR: File has no frontmatter
  → src/notes/random.md
```

### `frontmatter-valid`

```
ERROR: Invalid YAML in frontmatter
  → src/notes/broken.md
  Line 3: expected ':' but found '-'
```

### `type-required`

```
ERROR: Missing required property 'type'
  → src/work/notes/meeting.md
```

### `type-valid`

```
ERROR: Invalid type 'meeting'
  → src/notes/standup.md
  Valid types: index, person, organization, project, task, proposal,
               contract, note, snippet, workaround, walkthrough,
               research, reference, log, event
```

### `created-required`

```
ERROR: Missing required property 'created'
  → src/work/projects/new.md

FIX: Can auto-add using file modification time
```

### `inheritance`

```
ERROR: Missing inherited property 'area'
  → src/work/people/staff/profile.md
  Expected: area: work
  Defined in: src/work/index.md
```

The mechanism this rule enforces is under reconsideration; see
[Inheritance](../concepts/inheritance.md).

### `index-required`

```
ERROR: Folder missing index.md
  → src/work/projects/new_project/
```

### `allowed-types`

```
ERROR: Type 'project' not allowed in this folder
  → src/work/people/staff/some_project.md
  Allowed types: [person]
  Defined in: src/work/people/staff/index.md
```

### `archive-placement`

```
ERROR: File has archived=true but not in archive/
  → src/work/projects/old_project.md

FIX: Can move to src/work/archive/projects/old_project.md
```

### `orphaned-assets`

```
WARN: Asset not referenced by any markdown file
  → src/work/assets/screenshot.png
```

Links are protected and orphaned files are detected. *What becomes of an orphaned
file is not yet specified.* The rule can be slow on a large corpus.

### `embedded-tasks`

Each task belongs in its own document.

```
WARN: File contains 5 independent task items
  → src/product/notes/planning.md

  Consider splitting into individual task files.
```

### `inbox-raw`

```
INFO: Inbox file needs processing (no frontmatter)
  → src/inbox/quick_note.md

  Run 'arky inbox' to classify
```

See [arky inbox](cli/inbox.md).

## Rules for knowledge patterns

Three further rules cover the observation and relation notation described under
[Knowledge patterns](../concepts/knowledge-patterns.md). Each takes its level
from a configuration key rather than carrying a fixed one.

| Rule | Default level | What it checks |
|---|---|---|
| `observation-format` | Warning | An observation line follows the `- [category] content` form |
| `relation-target-exists` | Warning | A relation's target exists as a file |
| `relation-type-valid` | Information | A relation's type comes from the known set, or from the additions in the configuration |

```
WARN: Invalid observation format
  → src/work/projects/service.md:15
  Found: - [bad format
  Expected: - [category] content
```

```
WARN: Relation target not found
  → src/work/projects/service.md:22
  - depends_on [[Nonexistent Service]]
  No file found matching "Nonexistent Service"
```

```
INFO: Unknown relation type 'foobar'
  → src/work/projects/service.md:23
  Consider: depends_on, blocks, owned_by, related_to, part_of, ...
```

## Choosing which rules run

Each rule is turned on or off in the configuration file. The validation command
takes a rule filter, so a run can be narrowed to named rules, and the level at
which the run fails — error, warning, or any, with error as the default — which
decides the process [exit code](exit-codes.md). arky never modifies a file
without an explicit repair flag. See [arky check](cli/check.md) and
[arky fix](cli/fix.md).

## Not yet specified

- What the linter reports when a document fails a user-declared shape.
- Which rule names, levels, and messages survive once the rules follow from
  shapes.
- Where custom rules fit. The [configuration](config.md) sketches a custom-rule
  table and a hook table that runs an external command; an embedded scripting
  language is held back until it is needed.

See [Design status](../design-status.md) for the open questions around
validation.
