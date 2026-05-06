---
id: epo-d2-decide-fate-of-inherited-dirt-u94e
title: 'D2: decide fate of inherited dirty files (CLAUDE.md, AGENTS.md, .agents/vibe-context, .serena)'
type: chore
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 30
tags:
    - audit
    - linux-v1
    - repo-hygiene
description: Reviewer-visible noise. Each item is a one-line decision; defer until owner can choose.
intent: Resolve dangling repo state so a fresh clone matches what's pushed.
acceptance_criteria:
    - CLAUDE.md change either committed or reverted.
    - AGENTS.md (untracked) committed or .gitignore'd.
    - .agents/vibe-context/latest-crank-wave.json change either committed or reverted; entry added to .gitignore if it's tooling state.
    - .serena/ untracked tree either committed or .gitignore'd.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Reviewer-visible noise. Each item is a one-line decision; defer until owner can choose.

## Acceptance criteria

- CLAUDE.md change either committed or reverted.
- AGENTS.md (untracked) committed or .gitignore'd.
- .agents/vibe-context/latest-crank-wave.json change either committed or reverted; entry added to .gitignore if it's tooling state.
- .serena/ untracked tree either committed or .gitignore'd.
