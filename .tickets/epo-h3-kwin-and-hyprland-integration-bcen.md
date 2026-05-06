---
id: epo-h3-kwin-and-hyprland-integration-bcen
title: 'H3: KWin and Hyprland integration jobs in CI'
type: task
status: open
parent: epo-linux-v1-spec-gap-audit-2026-05--gm5i
priority: 70
tags:
    - audit
    - linux-v1
    - testing
description: Spec §13.5 calls these out as required because compositor-specific behavior is part of the shipping contract. Currently zero coverage.
intent: Add separate CI jobs that exercise the daemon end-to-end on KWin and Hyprland headless containers (spec §13.5).
acceptance_criteria:
    - linux-kwin-integration job runs ccvv-linux against a headless KWin session and asserts ext-data-control round-trip.
    - linux-hyprland-integration job does the same for current-release Hyprland.
    - Both jobs produce attributable failures when the compositor matrix breaks.
created: "2026-05-06T20:57:17Z"
extended_status: open
---
Spec §13.5 calls these out as required because compositor-specific behavior is part of the shipping contract. Currently zero coverage.

## Acceptance criteria

- linux-kwin-integration job runs ccvv-linux against a headless KWin session and asserts ext-data-control round-trip.
- linux-hyprland-integration job does the same for current-release Hyprland.
- Both jobs produce attributable failures when the compositor matrix breaks.
