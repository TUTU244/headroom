# Headroom agent hooks

This plugin exposes lightweight startup hooks for Claude Code and GitHub Copilot CLI.

The hooks call:

```bash
headroom init hook ensure
```

That hidden helper checks for a matching durable `headroom init` deployment and starts it if needed.

## Skills

- **refactoring** (`skills/refactoring/SKILL.md`) — guides safe, behavior-preserving
  refactors: deduplication, naming, splitting oversized functions/files, dead code
  removal, and keeping a test-backed safety net at every step.
