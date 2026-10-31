# Configuration

arky is configured by `arky.toml` files: the one at the root of the repository
it governs, and any other from the current folder up to the root of the file
system. The file names the stores arky reads and writes, and adjusts which rules
run and how they behave; everything else a corpus must satisfy comes from the
shapes that govern its documents.

## The file

```toml
default = "notes"           # the saved store used when none is named

[vault]
root = "src"
ignore = [".obsidian", "assets", "templates"]

[rules]
# Which rules to enable (all enabled by default)
frontmatter_required = true
frontmatter_valid = true
type_required = true
type_valid = true
created_required = true
inheritance = true
index_required = true
allowed_types = true
archive_placement = true
orphaned_assets = true      # Can be slow on large vaults
embedded_tasks = true

[rules.embedded_tasks]
min_items_to_warn = 3       # Only warn if >= 3 checkboxes

[resource_types]
valid = [
  "index",
  "person",
  "organization",
  "project",
  "task",
  "proposal",
  "contract",
  "note",
  "snippet",
  "workaround",
  "walkthrough",
  "research",
  "reference",
  "log",
  "event"
]

[inheritance]
# Properties that cascade from parent index.md
properties = ["area", "client", "role"]

[archive]
trigger_property = "archived"
folder_name = "archive"

[inbox]
path = "inbox"
allow_raw = true            # Inbox files can have no frontmatter

[assets]
extensions = ["png", "jpg", "jpeg", "gif", "svg", "pdf", "mp3", "opus", "mp4"]

[lock]
expires_seconds = 5         # how long a save's lock on a document lasts after it is taken
wait_seconds = 2            # how long a save waits on another save's lock before it is rejected

[stores.notes]
url = "file:notes"          # a store string; a relative path is relative to this file's folder

[stores.media]
url = "minio+https://${MINIO_USER}:${MINIO_PASSWORD}@minio.example.com/media"

[ai]
provider = "anthropic"      # or "openai"
model = "claude-sonnet-4-20250514"
# API key from environment: ANTHROPIC_API_KEY or OPENAI_API_KEY

[output]
# Default output format
format = "text"             # text, json, markdown
color = true
```

Knowledge-pattern recognition extends the same file:

```toml
[knowledge_patterns]
enabled = true

# Observation categories (extensible)
observation_categories = [
  "fact", "decision", "tip", "warning",
  "question", "todo", "insight", "reference"
]

# Relation types (extensible)
relation_types = [
  "depends_on", "blocks", "owned_by", "related_to",
  "part_of", "supersedes", "references", "implements", "extends"
]

# Validation levels
validate_observation_format = "warn"     # off, info, warn, error
validate_relation_targets = "warn"       # off, info, warn, error
validate_relation_types = "info"         # off, info, warn, error
```

## The keys and the tables

**`default`** — the name of the saved store `arky get` and `arky write` use when
their path is not a store string and neither `--store` nor `ARKY_STORE` names a
store: `default = "notes"` stands for `arky://notes`. It is a string; anything
else exits with code 3. A name that no `arky.toml` saves exits with code 3 when a
command uses it. With no `default`, those commands use the file system.

**`[stores.<name>]`** — a saved store: `url`, the one key the table takes, is a
[store string](cli/index.md#store-strings), and `arky://<name>` names that store
wherever a store is given, with a folder inside it after the name,
`arky://<name>/<folder>`. The `url` is any store string but `arky://`, so a saved
store never names another, and it may hold `${NAME}` variables, replaced each
time the store opens. A relative path in it, `file:notes` or `notes`, is relative
to the folder of the `arky.toml` that saves it, wherever arky runs. A
`stores.<name>` that is not a table, a `url` that is not a string, a missing
`url`, a key other than `url`, or a `url` that starts with `arky://` exits with
code 3, on one line that names the file. [`arky store list`](cli/store.md) lists the saved stores in use.

**`[vault]`** — `root` names the folder a corpus lives in, and `ignore` lists
folder names a run skips. Nothing about arky requires Obsidian, though the
default ignore list skips its folder. *Whether the table keeps this name is not
yet specified.*

**`[rules]`** — one boolean per rule, all rules enabled by default, with a nested
table for the rules that take settings of their own. `min_items_to_warn` is the
only one, and it sets how many checkbox items in a note raise the embedded-tasks
warning. See [Validation rules](validation-rules.md).

**`[resource_types]`** — `valid` lists the types a document may declare, read by
the `type-valid` rule. Resource types themselves are declared by remargin, and a
type can extend another to add more to it. *How user-declared shapes replace this
list is not yet specified.*

**`[inheritance]`** — `properties` lists the property names that cascade from a
parent folder's `index.md` to the files below it. The mechanism is under
reconsideration; see [Inheritance](../concepts/inheritance.md).

**`[archive]`** — `trigger_property` names the frontmatter property that marks a
document as archived, and `folder_name` names the folder an archived document
belongs in. Both are read by the `archive-placement` rule.

**`[inbox]`** — `path` names the inbox folder, and `allow_raw` permits files
there to have no frontmatter. See [arky inbox](cli/inbox.md).

**`[assets]`** — the file extensions a run treats as assets rather than as
documents, read by `orphaned-assets`.

**`[lock]`** — `expires_seconds` sets how long a save's lock on a document lasts
after it is taken, 5 by default, and `wait_seconds` sets how long a save waits on
another save's lock before it is rejected, 2 by default. Each is a whole number
of seconds above zero, and these two keys are the only ones the table takes: any
other key, or a value that is not a whole number above zero, exits with code 3.
The file store and the MinIO store take these settings; see
[arky write](cli/write.md). `default`, `[lock]` and `[stores.<name>]` are what
arky reads today; it leaves the other tables alone.

**`[ai]`** — the provider and model used when a language model classifies inbox
captures; the credential comes from the environment. Validation works offline,
and the language model is optional.

**`[output]`** — `format` sets the default output format, and `color` turns
colored output on or off. Both are overridable on the command line, and
`NO_COLOR` also turns color off. See [Output formats](output-formats.md).

**`[knowledge_patterns]`** — turns pattern recognition on or off, extends the
observation categories and the relation types, and sets each pattern rule's level
to off, information, warning, or error. See
[Knowledge patterns](../concepts/knowledge-patterns.md).

## Where arky finds the file

Every command reads the configuration when it starts. arky reads every
`arky.toml` from the current folder up to the root of the file system, and merges
them, the nearer file winning:

- A saved store comes whole from the nearest file that saves its name, so a
  vault's own `arky.toml` can give a name another meaning than the one in the
  home folder.
- `default` comes from the nearest file that sets it.
- Each key of `[lock]` comes from the nearest file that sets it, the other key
  from a farther file or its default.

A folder with no `arky.toml` adds nothing, and with none anywhere arky uses the
built-in defaults and has no saved store. An `arky.toml` that does not read, or
one of whose keys breaks a rule, exits with code 3, on one line that names it,
whatever the command.

`--config <FILE>` names one file, read alone in place of the walk, and
`ARKY_CONFIG` names one when `--config` names none. A relative path is taken
against the current folder. A file they name that does not exist exits with code
3.

```console
$ cat ~/arky.toml
default = "notes"

[stores.notes]
url = "file:notes"

[stores.media]
url = "minio+https://minio.example.com/media"
$ cat ~/work/arky.toml
[stores.notes]
url = "file:vault"

[stores.archive]
url = "file:archive"
$ arky get ideas.md
# Ideas at home
$ cd ~/work
$ arky store list
archive  file:archive                           /home/me/work/arky.toml
media    minio+https://minio.example.com/media  /home/me/arky.toml
notes    file:vault                             /home/me/work/arky.toml
$ arky get ideas.md
# Ideas at work
$ arky get ideas.md -c ~/arky.toml
# Ideas at home
$ ARKY_CONFIG=~/arky.toml arky get ideas.md
# Ideas at home
$ arky get ideas.md -c none.toml; echo "exit $?"
arky: none.toml: the configuration file does not exist
exit 3
```

From the home folder, `default` names `notes`, `~/notes`. From `~/work`, the same
`default` names the `notes` of `~/work/arky.toml`, `~/work/vault`, its `file:`
path relative to that file's folder.

## Precedence

Settings take precedence in this order, highest first:

1. Command-line options
2. Environment variables
3. `arky.toml`
4. Built-in defaults

## Environment variables

| Variable | What it does |
|---|---|
| `ARKY_CONFIG` | Path to the one configuration file read, in place of the `arky.toml` files from the current folder up |
| `ARKY_STORE` | The store `arky get` and `arky write` use when `--store` names none, in place of `default` |
| `ANTHROPIC_API_KEY` | Key for classifying inbox captures with a language model |
| `OPENAI_API_KEY` | The same, for the other provider |
| `NO_COLOR` | Turns colored output off |

The command line names the one configuration file with `--config`, in place of
`ARKY_CONFIG`.

## Extending the rule set

Rule extension needs neither WebAssembly nor an embedded Lua runtime; two tables
cover it. A custom-rule table names a type, the properties that type requires,
and the message to print. A hook table names an external command to run as part
of a check. An embedded scripting language is held back until it is needed.

## Not yet specified

- How a shape or a schema is attached to the documents it governs, and what a
  user-declared [shape](shape-language.md) leaves for a custom rule to do.
- Which storage backend holds a corpus and its versions, and anything about
  change objects.
- Anything about the [Model Context Protocol server](mcp/index.md).

See [Design status](../design-status.md) for the open questions around
configuration.
