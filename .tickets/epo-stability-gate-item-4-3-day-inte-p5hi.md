---
id: epo-stability-gate-item-4-3-day-inte-p5hi
title: 'Stability gate item 4: 3-day internal soak with no P1/P2 regressions'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 60
tags:
    - audit
    - linux-v1
    - human-required
description: Spec §13.7 + hardening plan release-gate item 4. Fundamentally a human dogfooding task.
intent: Run the daemon for at least 3 active calendar days under normal use and confirm no copy/detect/format regression.
acceptance_criteria:
    - Soak duration documented in the hardening plan with start/end timestamps.
    - Any observed regression filed as a separate ticket and either resolved or accepted before declaring 'wider invite'.
created: "2026-05-06T20:57:55Z"
extended_status: open
---
Spec §13.7 + hardening plan release-gate item 4. Fundamentally a human dogfooding task.

## Acceptance criteria

- Soak duration documented in the hardening plan with start/end timestamps.
- Any observed regression filed as a separate ticket and either resolved or accepted before declaring 'wider invite'.
