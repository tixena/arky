+++
type = "task"
title = "Write the body lexer"
status = "in_progress"
priority = 1
estimate_hours = 6.5
done = false
due = 2026-10-10
created = 2026-10-03T14:29:13-04:00
standup_at = 09:30:00
assignees = ["ada", "grace"]
blocked_by = []
+++
# Write the body lexer

Turn body lines into tokens. Depends on [[tree]].

## Checklist

- [x] Blank and text lines
- [x] Headings
- [ ] List items
  - [ ] nested items
  - [ ] continuation lines
- [ ] Fenced code blocks

## Notes

Indentation is counted in spaces only.
