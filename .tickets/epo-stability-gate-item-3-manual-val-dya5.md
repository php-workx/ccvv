---
id: epo-stability-gate-item-3-manual-val-dya5
title: 'Stability gate item 3: manual validation matrix completion'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 60
tags:
    - audit
    - linux-v1
    - human-required
description: Cannot be automated. Requires a human on real hardware copying real content from browsers, terminals, IDEs, PDFs, mail clients.
intent: Fill docs/plans/2026-03-11-linux-stability-validation-matrix.md by exercising the high-risk copy/detect/format flows on real X11, wlroots Wayland, and GNOME Wayland sessions.
acceptance_criteria:
    - All three rows in the matrix have Pass/Fail entries with notes for each scenario column.
    - Build SHA + tester + date recorded in run metadata.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Cannot be automated. Requires a human on real hardware copying real content from browsers, terminals, IDEs, PDFs, mail clients.

## Acceptance criteria

- All three rows in the matrix have Pass/Fail entries with notes for each scenario column.
- Build SHA + tester + date recorded in run metadata.
