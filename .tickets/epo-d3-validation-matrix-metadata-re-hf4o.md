---
id: epo-d3-validation-matrix-metadata-re-hf4o
title: 'D3: validation matrix metadata reflects automated coverage that already passes'
type: doc
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 30
tags:
    - audit
    - linux-v1
    - docs
description: Today the matrix is fully blank; reviewer can't tell which rows are 'pending human' vs 'pending CI investment'.
intent: Augment docs/plans/2026-03-11-linux-stability-validation-matrix.md so reviewers see what is already automated vs what genuinely needs a human.
acceptance_criteria:
    - Per-row notes call out that wlroots Wayland is partially covered by the headless harness, while X11 and GNOME limited rows still need human evidence.
    - Build SHA + automation date populated from the latest passing CI run.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Today the matrix is fully blank; reviewer can't tell which rows are 'pending human' vs 'pending CI investment'.

## Acceptance criteria

- Per-row notes call out that wlroots Wayland is partially covered by the headless harness, while X11 and GNOME limited rows still need human evidence.
- Build SHA + automation date populated from the latest passing CI run.
