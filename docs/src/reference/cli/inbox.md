# arky inbox

`arky inbox` reads the captures dropped into a corpus without being filed, asks
a language model to classify each one, and proposes where each should go.

## Synopsis

```console
$ arky inbox [PATH]
```

`[PATH]` is the inbox path; optional, defaulting from the configuration file.

## Options

| Option | Effect |
|---|---|
| `--dry-run` | Show the proposals without applying them. |
| `--auto` | Apply the proposals without asking. Dangerous. |
| `--provider <NAME>` | The language-model provider: `anthropic` or `openai`. Default from the configuration file; credential from `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`. |
| `--model <MODEL>` | The model to use. Default from the configuration file. |

## Behavior

A run has five steps: scan the inbox for unprocessed files; ask the model to
classify each one; show what it proposes — a type, an area, properties, and a
destination path; let the person approve, edit, skip, or reject; and apply what
was approved. An unprocessed file is one with no frontmatter; the validation run
reports it under the `inbox-raw` rule, and raw files are allowed in the inbox so
this is not an error there. The inbox is a global landing zone for quick
captures, imported items, and notes filed later; each area may have its own.

## Examples

```console
$ arky inbox src/inbox/ --dry-run
```

The interactive screen:
```console
$ arky inbox
Processing: src/inbox/quick_note.md

Content:
  "Fix the login timeout issue in the web app
   The session expires after 5 minutes which is too short.
   Users are complaining."

AI Proposal:
  Type: task
  Area: product
  Filename: fix_login_timeout.md
  Priority: high
  Tags: authentication, ux
  Destination: src/product/tasks/fix_login_timeout.md

[A]pprove  [E]dit  [S]kip  [R]eject  [Q]uit
>
```

The run stops until one of the five letters is given. The same flow also proposes
the observations and relations found in a capture's text:

```console
$ arky inbox
Processing: src/inbox/meeting_notes.md

AI Proposal:
  Type: note
  Area: work

  Suggested Observations:
    - [decision] Moving deployment to Kubernetes
    - [todo] Schedule follow-up with DevOps team
    - [fact] Current deployment takes 45 minutes

  Suggested Relations:
    - related_to [[Kubernetes Migration]]
    - attendees [[Platform Team]] [[Design Team]]

[A]pprove  [E]dit  [S]kip  [R]eject  [Q]uit
```

`attendees` is not among the nine known relation types, so this proposal names
one the relation-type rule reports as unknown; see
[Knowledge patterns](../../concepts/knowledge-patterns.md).

## Not yet specified

- What makes a proposal acceptable, what the edit choice can change, what happens
  to a rejected file, what the screen shows after a choice, and what the run
  prints when it finishes.
- Which credential variable wins when both are set, and every exit code but the
  language-model provider error.

See [Design status](../../design-status.md) for the open questions here.

## Related

- [arky import](import.md) — the other route items reach the inbox by.
- [arky check](check.md) — the `inbox-raw` rule.
- [Configuration](../config.md) — the inbox path, the provider, and the model.
- [Command-line interface](index.md) — global options and the command table.
